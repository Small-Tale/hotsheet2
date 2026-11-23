//! HS2-7DBJMS: server-owned remote pairing and revocable browser sessions.
//!
//! The file contains only SHA-256 digests of random credentials. Every transition writes
//! atomically before publishing the new in-memory state, so a crash cannot grant a session
//! that the next process does not know how to revoke.
//!
//! HS2-85W6BE / HS2-DXM0YN: a session carries two grants besides its read role. `admin`
//! lets the browser manage devices; `local` marks the browser as the host owner's own
//! (host-trusted: folder chooser, open by path, setup). Neither is derived from the URL,
//! peer address or forwarded headers, because Tailscale Serve reaches the host over the
//! same loopback port the owner opens locally.
//!
//! HS2-4B0KSD (maintainer policy): a session expires after [`IDLE_TTL`] (30 days) without use,
//! or [`SESSION_TTL`] (90 days) after its approval or last renewal, whichever comes first. Use
//! refreshes only the idle clock, persisted at most once per [`ACTIVITY_PERSIST_INTERVAL`];
//! open streams count as use because the WebSocket and terminal routes recheck their session
//! every second through [`RemoteAuth::authorize`]. Renewal happens in place
//! ([`RemoteAuth::renew`]): the device keeps its id, credential, role and grants, so a desktop
//! binding and its selected projects keep working. Reminders fall on [`reminder_day`]'s days.
//! A revoked session leaves a tombstone so it fails as revoked, not as unknown, without an
//! older server ever accepting it again.
//!
//! HS2-YDDPN4: one catalog serves the whole machine, `${HOTSHEET_HOME}/remote-auth/machine.json`
//! ([`RemoteAuth::open_machine`]). A paired device authorizes this machine's server, not one
//! primary store, so a server started on another primary store keeps every device. The first
//! open merges the earlier per-store catalogs (`remote-auth/<store-id>.json`) into it, under the
//! catalog's file lock, and leaves those files in place (see [`migration`]). Servers on several
//! primary stores may run at once: every write re-reads the file under that lock, and every read
//! reloads the file when another process replaced it, so a revocation anywhere holds everywhere.

mod migration;

pub use migration::MigrationReport;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

/// HS2-DXM0YN: long enough to walk to the host and approve.
pub const PAIR_TTL: i64 = 15 * 60;
const DAY: i64 = 24 * 60 * 60;
/// The absolute lifetime, counted from approval or the last renewal; use never extends it.
pub const SESSION_TTL: i64 = 90 * DAY;
/// A device that makes no authenticated request, and holds no open stream, this long expires.
pub const IDLE_TTL: i64 = 30 * DAY;
/// The days after approval or renewal on which a renewal reminder appears (HS2-4B0KSD):
/// 45, 50, 55 and 60, then every day from 65 through 90.
pub const REMINDER_DAYS: [i64; 30] = [
    45, 50, 55, 60, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84,
    85, 86, 87, 88, 89, 90,
];
/// A revoked session's tombstone prefix. An older server compares digests only, so a
/// tombstone never matches a presented token there either.
const REVOKED_PREFIX: &str = "revoked:";
/// Persist idle activity at most this often per session, so busy reads and the per-second
/// stream rechecks cost one catalog write a minute (HS2-4B0KSD).
pub const ACTIVITY_PERSIST_INTERVAL: i64 = 60;
const MAX_ATTEMPTS: u8 = 5;
const MAX_REQUESTS_PER_MINUTE: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    ReadOnly,
    ReadWrite,
}

/// The grants an approval or a later change gives a device besides its read role.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Grants {
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub local: bool,
}

/// A partial change to one paired device; `None` keeps the current value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceChange {
    pub role: Option<Role>,
    pub admin: Option<bool>,
    pub local: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Pairing {
    pub id: String,
    pub server_id: String,
    pub challenge: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Device {
    pub id: String,
    pub server_id: String,
    pub label: String,
    pub role: Role,
    pub admin: bool,
    pub local: bool,
    pub origin: String,
    pub created_at: i64,
    pub last_used_at: i64,
    /// The absolute end: `renewed_at` plus [`SESSION_TTL`].
    pub expires_at: i64,
    /// Approval or the last renewal; the reminder and absolute clocks count from it.
    pub renewed_at: i64,
    /// When the idle clock runs out unless the device is used first.
    pub idle_expires_at: i64,
    /// The device asked the host to renew it and the host has not yet.
    pub renewal_requested_at: Option<i64>,
    /// The latest reminder day reached (see [`REMINDER_DAYS`]); filled by [`RemoteAuth::list`]
    /// and the session routes for the time they answer.
    pub reminder_day: Option<i64>,
    /// Already expired, by either clock; only [`RemoteAuth::list`] reports such a device.
    pub expired: bool,
}

/// The latest reminder day reached at `now` for a session renewed at `renewed_at`, or `None`
/// before day 45 (HS2-4B0KSD). Clients compute the same days from the same timestamps.
pub fn reminder_day(renewed_at: i64, now: i64) -> Option<i64> {
    let age = now.saturating_sub(renewed_at);
    REMINDER_DAYS
        .iter()
        .rev()
        .find(|day| age >= **day * DAY)
        .copied()
}

#[derive(Clone, Debug, Serialize)]
pub struct PendingDevice {
    pub id: String,
    pub label: String,
    pub origin: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub approved: bool,
}

#[derive(Clone, Debug)]
pub struct IssuedSession {
    pub token: String,
    pub csrf: String,
    pub device: Device,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PendingRecord {
    id: String,
    label: String,
    origin: String,
    challenge_hash: String,
    created_at: i64,
    expires_at: i64,
    attempts: u8,
    approved_role: Option<Role>,
    #[serde(default)]
    approved_grants: Grants,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SessionRecord {
    device: DeviceRecord,
    token_hash: String,
    csrf_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DeviceRecord {
    id: String,
    server_id: String,
    label: String,
    role: Role,
    #[serde(default)]
    admin: bool,
    #[serde(default)]
    local: bool,
    origin: String,
    created_at: i64,
    last_used_at: i64,
    /// Kept equal to `renewed_at + SESSION_TTL` so an older server enforces the same end.
    expires_at: i64,
    /// Absent in catalogs written before HS2-4B0KSD: the approval time counts.
    #[serde(default)]
    renewed_at: Option<i64>,
    #[serde(default)]
    renewal_requested_at: Option<i64>,
}

impl DeviceRecord {
    fn renewed(&self) -> i64 {
        self.renewed_at.unwrap_or(self.created_at)
    }
    fn expires(&self) -> i64 {
        self.renewed() + SESSION_TTL
    }
    fn idle_expires(&self) -> i64 {
        self.last_used_at + IDLE_TTL
    }
    fn expired(&self, now: i64) -> bool {
        self.expires() <= now || self.idle_expires() <= now
    }
}

impl From<&DeviceRecord> for Device {
    fn from(value: &DeviceRecord) -> Self {
        Self {
            id: value.id.clone(),
            server_id: value.server_id.clone(),
            label: value.label.clone(),
            role: value.role,
            admin: value.admin,
            local: value.local,
            origin: value.origin.clone(),
            created_at: value.created_at,
            last_used_at: value.last_used_at,
            expires_at: value.expires(),
            renewed_at: value.renewed(),
            idle_expires_at: value.idle_expires(),
            renewal_requested_at: value.renewal_requested_at,
            reminder_day: None,
            expired: false,
        }
    }
}

impl Device {
    /// The same device with the reminder and expiry state at `now`.
    #[must_use]
    pub fn at(mut self, now: i64) -> Self {
        self.reminder_day = reminder_day(self.renewed_at, now);
        self.expired = self.expires_at <= now || self.idle_expires_at <= now;
        self
    }
}

impl SessionRecord {
    fn revoked(&self) -> bool {
        self.token_hash.starts_with(REVOKED_PREFIX)
    }
}

/// The live or revoked session `hash` names.
fn find_session(sessions: &[SessionRecord], hash: &str) -> Option<usize> {
    let tombstone = format!("{REVOKED_PREFIX}{hash}");
    sessions.iter().position(|entry| {
        equal_digest(&entry.token_hash, hash) || equal_digest(&entry.token_hash, &tombstone)
    })
}

/// Turn a session into its tombstone: no credential matches it again.
fn tombstone(entry: &mut SessionRecord, now: i64) {
    if !entry.revoked() {
        entry.token_hash = format!("{REVOKED_PREFIX}{}", entry.token_hash);
        entry.csrf_hash = format!("{REVOKED_PREFIX}{}", entry.csrf_hash);
    }
    // The tombstone is kept for one lifetime after revocation, then pruned.
    entry.device.last_used_at = now;
    entry.device.renewed_at = Some(now);
    entry.device.expires_at = now + SESSION_TTL;
}

/// Drop tombstones and long-expired sessions once their lifetime has passed again, so the
/// catalog stays bounded by the devices in use.
fn prune(sessions: &mut Vec<SessionRecord>, now: i64) {
    sessions.retain(|entry| {
        let end = if entry.revoked() {
            entry.device.expires()
        } else {
            entry.device.expires().min(entry.device.idle_expires())
        };
        end + SESSION_TTL > now
    });
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    server_id: String,
    pending: Vec<PendingRecord>,
    sessions: Vec<SessionRecord>,
    #[serde(default)]
    recent_requests: Vec<i64>,
    /// The per-store catalogs already merged into the machine catalog (HS2-YDDPN4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    migrated: Vec<migration::MigratedSource>,
}

impl Data {
    fn new(server_id: &str) -> Self {
        Self {
            server_id: server_id.to_owned(),
            pending: Vec::new(),
            sessions: Vec::new(),
            recent_requests: Vec::new(),
            migrated: Vec::new(),
        }
    }
}

/// The machine catalog's directory under `${HOTSHEET_HOME}`; it also holds the per-store
/// catalogs written before HS2-YDDPN4.
pub const CATALOG_DIR: &str = "remote-auth";
/// The machine catalog's file name inside [`CATALOG_DIR`].
pub const MACHINE_CATALOG: &str = "machine.json";

/// The machine-scoped session catalog under `home` (HS2-YDDPN4).
pub fn machine_catalog_path(home: &Path) -> PathBuf {
    home.join(CATALOG_DIR).join(MACHINE_CATALOG)
}

/// The catalog in memory and the file version it was read from.
struct Loaded {
    data: Data,
    stamp: Option<Stamp>,
}

/// Identifies one version of the catalog file. Every write replaces the file, so a write by any
/// process changes the inode (Unix), the length or the modification time. Reads use it to skip
/// re-parsing an unchanged file; writes re-read regardless. A revocation always changes the
/// length, because its tombstone prefixes both digests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Stamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    inode: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(&metadata);
    #[cfg(not(unix))]
    let inode = 0;
    Some(Stamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        inode,
    })
}

/// Read and check the catalog at `path`; `None` when it does not exist yet. A corrupt catalog
/// or one bound to another server identity is an error, never an empty catalog, so no
/// revocation is silently forgotten.
fn read_catalog(path: &Path, server_id: &str) -> Result<Option<Data>> {
    let data: Data = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("invalid remote auth catalog")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("read remote auth catalog"),
    };
    if data.server_id != server_id {
        anyhow::bail!("remote auth catalog belongs to another server identity");
    }
    Ok(Some(data))
}

/// Replace the in-memory catalog with `fresh`, read from disk, without moving any live
/// session's idle clock backward: a file replaced by an older copy never makes a device that
/// this process saw in use look idle (HS2-YDDPN4).
fn adopt(current: &mut Data, mut fresh: Data) {
    for entry in &mut fresh.sessions {
        if let Some(known) = current.sessions.iter().find(|known| {
            known.device.id == entry.device.id && equal_digest(&known.token_hash, &entry.token_hash)
        }) {
            entry.device.last_used_at = entry.device.last_used_at.max(known.device.last_used_at);
        }
    }
    *current = fresh;
}

/// Hold the catalog's cross-process lock (`<catalog>.lock`) until the result is dropped.
fn lock_catalog(path: &Path) -> Result<hotsheet_ticketing::FileLock> {
    let mut lock = path.as_os_str().to_owned();
    lock.push(".lock");
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(PathBuf::from(lock))
        .context("open remote auth catalog lock")?;
    hotsheet_ticketing::FileLock::acquire(file).context("lock remote auth catalog")
}

/// The seconds clock the routes read for every session decision (HS2-4B0KSD).
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The wall clock in Unix seconds.
pub fn wall_clock() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// A settable clock for transition tests (HS2-4B0KSD): `clock()` hands it to
/// [`RemoteAuth::with_clock`], and `set` or `advance` moves every reader at once.
#[derive(Clone, Default)]
pub struct ManualClock(Arc<AtomicI64>);

impl ManualClock {
    pub fn new(start: i64) -> Self {
        Self(Arc::new(AtomicI64::new(start)))
    }
    pub fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    pub fn set(&self, value: i64) {
        self.0.store(value, Ordering::SeqCst);
    }
    pub fn advance(&self, seconds: i64) -> i64 {
        self.0.fetch_add(seconds, Ordering::SeqCst) + seconds
    }
    pub fn clock(&self) -> Clock {
        let value = self.0.clone();
        Arc::new(move || value.load(Ordering::SeqCst))
    }
}

#[derive(Clone)]
pub struct RemoteAuth {
    path: PathBuf,
    data: Arc<Mutex<Loaded>>,
    /// What opening merged from the per-store catalogs (HS2-YDDPN4).
    migration: Arc<MigrationReport>,
    /// In-memory change cursor for device-list long polls. It starts from the clock so a
    /// restarted server never repeats a cursor a client already holds.
    revision: Arc<tokio::sync::watch::Sender<u64>>,
    clock: Clock,
    /// How many times this process wrote the catalog; tests bound the activity write rate.
    writes: Arc<tokio::sync::watch::Sender<u64>>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AuthFailure {
    Invalid,
    /// A pairing challenge ran out (HS2-7DBJMS).
    Expired,
    /// A paired session reached its idle or absolute limit; renewal restores it (HS2-4B0KSD).
    SessionExpired,
    /// A paired session was revoked; only a new pairing grants access again.
    Revoked,
    RateLimited,
    Unapproved,
    ReadOnly,
    Csrf,
}

impl RemoteAuth {
    /// Open the catalog at `path` alone, without merging any per-store catalog (tests and
    /// single-file callers).
    pub fn open(path: impl Into<PathBuf>, server_id: &str) -> Result<Self> {
        Self::open_with(path.into(), server_id, None, wall_clock())
    }

    /// Open this machine's catalog under `home` and merge the per-store catalogs beside it
    /// (HS2-YDDPN4).
    pub fn open_machine(home: &Path, server_id: &str) -> Result<Self> {
        Self::open_machine_at(home, server_id, wall_clock())
    }

    /// [`Self::open_machine`] at `now`, which only decides which pending pairings are still
    /// worth carrying over.
    pub fn open_machine_at(home: &Path, server_id: &str, now: i64) -> Result<Self> {
        let path = machine_catalog_path(home);
        let legacy = home.join(CATALOG_DIR);
        Self::open_with(path, server_id, Some(&legacy), now)
    }

    fn open_with(path: PathBuf, server_id: &str, legacy: Option<&Path>, now: i64) -> Result<Self> {
        let parent = path.parent().context("remote auth catalog has no parent")?;
        fs::create_dir_all(parent)?;
        let lock = lock_catalog(&path)?;
        let existing = read_catalog(&path, server_id)?;
        let fresh = existing.is_none();
        let mut data = existing.unwrap_or_else(|| Data::new(server_id));
        let report = legacy.map_or_else(MigrationReport::default, |dir| {
            migration::migrate(&mut data, dir, &path, now)
        });
        if fresh || report.changed {
            write_private_atomic(&path, &serde_json::to_vec(&data)?)?;
        }
        let stamp = stamp(&path);
        drop(lock);
        for warning in &report.warnings {
            tracing::warn!("remote auth catalog migration: {warning}");
        }
        let start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        Ok(Self {
            path,
            data: Arc::new(Mutex::new(Loaded { data, stamp })),
            migration: Arc::new(report),
            revision: Arc::new(tokio::sync::watch::Sender::new(start)),
            clock: Arc::new(wall_clock),
            writes: Arc::new(tokio::sync::watch::Sender::new(0)),
        })
    }

    /// The catalog file this process reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What opening merged from the per-store catalogs, with every warning it recorded.
    pub fn migration(&self) -> &MigrationReport {
        &self.migration
    }

    /// The catalog as of now: reloaded first when another process replaced the file since this
    /// process last read or wrote it, so a revocation made by another server holds here at the
    /// next request. A reload counts as a device-list change. An unreadable replacement keeps
    /// the last good catalog and is logged once.
    fn current(&self) -> std::sync::MutexGuard<'_, Loaded> {
        let mut guard = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let on_disk = stamp(&self.path);
        if on_disk != guard.stamp {
            guard.stamp = on_disk;
            match read_catalog(&self.path, &guard.data.server_id) {
                Ok(Some(data)) => {
                    adopt(&mut guard.data, data);
                    self.revision
                        .send_modify(|revision| *revision = revision.wrapping_add(1));
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        "remote auth catalog reload failed; keeping the last good copy: {error:#}"
                    );
                }
            }
        }
        guard
    }

    /// Replace the clock the routes read (HS2-4B0KSD transition tests).
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// Now, in Unix seconds, by this catalog's clock.
    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    /// The number of catalog writes this process made, as a watch so tests wait on it.
    pub fn writes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.writes.subscribe()
    }

    /// The device-list change cursor.
    pub fn revision(&self) -> u64 {
        *self.revision.borrow()
    }

    /// Wait until the device list changes from `after`, or until `limit` passes.
    pub async fn changed_since(&self, after: u64, limit: std::time::Duration) {
        let mut receiver = self.revision.subscribe();
        let _ = tokio::time::timeout(limit, receiver.wait_for(|current| *current != after)).await;
    }

    pub fn server_id(&self) -> String {
        self.data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .data
            .server_id
            .clone()
    }

    /// A change people see in the device list: wakes every waiting long poll.
    fn update<T>(&self, change: impl FnOnce(&mut Data) -> Result<T>) -> Result<T> {
        let result = self.write(change)?;
        self.revision
            .send_modify(|revision| *revision = revision.wrapping_add(1));
        Ok(result)
    }

    /// A bookkeeping write (idle activity, credential rotation) that no list shows changing.
    ///
    /// The change applies to the file's latest content, read under the catalog's cross-process
    /// lock, so two servers sharing the machine catalog never overwrite each other's changes
    /// (HS2-YDDPN4). A catalog that cannot be read fails the write rather than being replaced.
    fn write<T>(&self, change: impl FnOnce(&mut Data) -> Result<T>) -> Result<T> {
        let mut guard = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _lock = lock_catalog(&self.path)?;
        // Always re-read under the lock: a stamp can repeat on a coarse-clock filesystem
        // that reuses inodes, and a stale base would lose another server's change.
        if let Some(data) = read_catalog(&self.path, &guard.data.server_id)? {
            adopt(&mut guard.data, data);
        }
        let mut next = guard.data.clone();
        let result = change(&mut next)?;
        write_private_atomic(&self.path, &serde_json::to_vec(&next)?)?;
        guard.stamp = stamp(&self.path);
        guard.data = next;
        self.writes.send_modify(|count| *count += 1);
        Ok(result)
    }

    pub fn request(
        &self,
        label: &str,
        origin: &str,
        now: i64,
    ) -> Result<Result<Pairing, AuthFailure>> {
        let label = label.trim();
        if label.is_empty() || label.len() > 80 || label.chars().any(char::is_control) {
            return Ok(Err(AuthFailure::Invalid));
        }
        if !valid_origin(origin) {
            return Ok(Err(AuthFailure::Invalid));
        }
        self.update(|data| {
            data.pending.retain(|entry| entry.expires_at > now);
            data.recent_requests.retain(|time| *time > now - 60);
            if data.recent_requests.len() >= MAX_REQUESTS_PER_MINUTE {
                return Ok(Err(AuthFailure::RateLimited));
            }
            data.recent_requests.push(now);
            let challenge = random_token()?;
            let pairing = Pairing {
                id: Ulid::new().to_string(),
                server_id: data.server_id.clone(),
                challenge,
                expires_at: now + PAIR_TTL,
            };
            data.pending.push(PendingRecord {
                id: pairing.id.clone(),
                label: label.to_owned(),
                origin: origin.to_owned(),
                challenge_hash: digest(&pairing.challenge),
                created_at: now,
                expires_at: pairing.expires_at,
                attempts: 0,
                approved_role: None,
                approved_grants: Grants::default(),
            });
            Ok(Ok(pairing))
        })
    }

    pub fn pending(&self, now: i64) -> Vec<PendingDevice> {
        self.current()
            .data
            .pending
            .iter()
            .filter(|entry| entry.expires_at > now)
            .map(|entry| PendingDevice {
                id: entry.id.clone(),
                label: entry.label.clone(),
                origin: entry.origin.clone(),
                created_at: entry.created_at,
                expires_at: entry.expires_at,
                approved: entry.approved_role.is_some(),
            })
            .collect()
    }

    /// Called only after the route authorized a trusted host owner or an admin session, and
    /// granted `local` only for a host-trusted caller.
    pub fn approve(
        &self,
        id: &str,
        role: Role,
        grants: Grants,
        now: i64,
    ) -> Result<Result<(), AuthFailure>> {
        self.update(|data| {
            let Some(entry) = data.pending.iter_mut().find(|entry| entry.id == id) else {
                return Ok(Err(AuthFailure::Invalid));
            };
            if entry.expires_at <= now {
                return Ok(Err(AuthFailure::Expired));
            }
            if entry.attempts >= MAX_ATTEMPTS {
                return Ok(Err(AuthFailure::RateLimited));
            }
            entry.approved_role = Some(role);
            entry.approved_grants = grants;
            Ok(Ok(()))
        })
    }

    /// Refuse one pending request; its browser's next redeem fails as invalid.
    pub fn reject(&self, id: &str) -> Result<bool> {
        self.update(|data| {
            let before = data.pending.len();
            data.pending.retain(|entry| entry.id != id);
            Ok(before != data.pending.len())
        })
    }

    /// Change one paired device's role or grants. Returns `None` when no such device exists.
    pub fn change(&self, id: &str, change: DeviceChange) -> Result<Option<Device>> {
        self.update(|data| {
            let Some(entry) = data
                .sessions
                .iter_mut()
                .find(|entry| entry.device.id == id && !entry.revoked())
            else {
                return Ok(None);
            };
            if let Some(role) = change.role {
                entry.device.role = role;
            }
            if let Some(admin) = change.admin {
                entry.device.admin = admin;
            }
            if let Some(local) = change.local {
                entry.device.local = local;
            }
            Ok(Some(Device::from(&entry.device)))
        })
    }

    pub fn redeem(
        &self,
        id: &str,
        challenge: &str,
        origin: &str,
        now: i64,
    ) -> Result<Result<IssuedSession, AuthFailure>> {
        self.update(|data| {
            let Some(position) = data.pending.iter().position(|entry| entry.id == id) else {
                return Ok(Err(AuthFailure::Invalid));
            };
            let entry = &mut data.pending[position];
            if entry.expires_at <= now {
                return Ok(Err(AuthFailure::Expired));
            }
            if entry.attempts >= MAX_ATTEMPTS {
                return Ok(Err(AuthFailure::RateLimited));
            }
            if !equal_digest(&entry.challenge_hash, &digest(challenge)) {
                entry.attempts += 1;
                return Ok(Err(AuthFailure::Invalid));
            }
            if entry.origin != origin {
                entry.attempts += 1;
                return Ok(Err(AuthFailure::Invalid));
            }
            let Some(role) = entry.approved_role else {
                return Ok(Err(AuthFailure::Unapproved));
            };
            let grants = entry.approved_grants;
            let entry = data.pending.remove(position);
            let token = random_token()?;
            let csrf = random_token()?;
            let device = DeviceRecord {
                id: Ulid::new().to_string(),
                server_id: data.server_id.clone(),
                label: entry.label,
                role,
                admin: grants.admin,
                local: grants.local,
                origin: entry.origin,
                created_at: now,
                last_used_at: now,
                expires_at: now + SESSION_TTL,
                renewed_at: Some(now),
                renewal_requested_at: None,
            };
            prune(&mut data.sessions, now);
            data.sessions.push(SessionRecord {
                device: device.clone(),
                token_hash: digest(&token),
                csrf_hash: digest(&csrf),
            });
            Ok(Ok(IssuedSession {
                token,
                csrf,
                device: Device::from(&device).at(now),
            }))
        })
    }

    pub fn authorize(
        &self,
        token: &str,
        csrf: Option<&str>,
        origin: Option<&str>,
        write: bool,
        now: i64,
    ) -> Result<Result<Device, AuthFailure>> {
        // Avoid a disk write for every API read and every per-second stream recheck: persist
        // activity at most once per interval. The inner recheck still makes concurrent
        // revocation authoritative.
        let hash = digest(token);
        {
            let guard = self.current();
            let Some(position) = find_session(&guard.data.sessions, &hash) else {
                return Ok(Err(AuthFailure::Invalid));
            };
            let entry = &guard.data.sessions[position];
            let result = validate(entry, csrf, origin, write, now);
            if result.is_err() || entry.device.last_used_at > now - ACTIVITY_PERSIST_INTERVAL {
                return Ok(result.map(|device| device.at(now)));
            }
        }
        self.write(|data| {
            let Some(position) = find_session(&data.sessions, &hash) else {
                return Ok(Err(AuthFailure::Invalid));
            };
            let entry = &mut data.sessions[position];
            if let Err(error) = validate(entry, csrf, origin, write, now) {
                return Ok(Err(error));
            }
            touch(&mut entry.device, now);
            Ok(Ok(Device::from(&entry.device).at(now)))
        })
    }

    /// Every paired device the host may manage, expired ones included so they can be renewed
    /// in place. Revoked tombstones are never listed.
    pub fn list(&self, now: i64) -> Vec<Device> {
        self.current()
            .data
            .sessions
            .iter()
            .filter(|entry| !entry.revoked())
            .map(|entry| Device::from(&entry.device).at(now))
            .collect()
    }

    /// Restart one device's lifetime in place (HS2-4B0KSD), before or after it expired. The
    /// device keeps its id, credential, role and grants, so nothing on the device needs setting
    /// up again. Returns `None` for an unknown or revoked device.
    pub fn renew(&self, id: &str, now: i64) -> Result<Option<Device>> {
        self.update(|data| {
            let Some(entry) = data
                .sessions
                .iter_mut()
                .find(|entry| entry.device.id == id && !entry.revoked())
            else {
                return Ok(None);
            };
            entry.device.renewed_at = Some(now);
            entry.device.expires_at = now + SESSION_TTL;
            entry.device.last_used_at = entry.device.last_used_at.max(now);
            entry.device.renewal_requested_at = None;
            Ok(Some(Device::from(&entry.device).at(now)))
        })
    }

    /// The device holding `token` asks the host to renew it (HS2-4B0KSD). An expired session
    /// may ask; a revoked or unknown one may not. The request needs the session's paired Origin
    /// and CSRF proof and is idempotent: a repeated request keeps the first time.
    pub fn request_renewal(
        &self,
        token: &str,
        csrf: &str,
        origin: &str,
        now: i64,
    ) -> Result<Result<Device, AuthFailure>> {
        let hash = digest(token);
        let current = {
            let guard = self.current();
            let Some(position) = find_session(&guard.data.sessions, &hash) else {
                return Ok(Err(AuthFailure::Invalid));
            };
            let entry = &guard.data.sessions[position];
            if entry.revoked() {
                return Ok(Err(AuthFailure::Revoked));
            }
            if entry.device.origin != origin {
                return Ok(Err(AuthFailure::Invalid));
            }
            if !equal_digest(&entry.csrf_hash, &digest(csrf)) {
                return Ok(Err(AuthFailure::Csrf));
            }
            entry
                .device
                .renewal_requested_at
                .map(|_| Device::from(&entry.device).at(now))
        };
        if let Some(device) = current {
            return Ok(Ok(device));
        }
        self.update(|data| {
            let Some(position) = find_session(&data.sessions, &hash) else {
                return Ok(Err(AuthFailure::Invalid));
            };
            let entry = &mut data.sessions[position];
            if entry.revoked() {
                return Ok(Err(AuthFailure::Revoked));
            }
            entry.device.renewal_requested_at.get_or_insert(now);
            Ok(Ok(Device::from(&entry.device).at(now)))
        })
    }

    pub fn revoke(&self, id: &str) -> Result<bool> {
        let now = self.now();
        self.update(|data| {
            let mut found = false;
            for entry in &mut data.sessions {
                if entry.device.id == id && !entry.revoked() {
                    tombstone(entry, now);
                    found = true;
                }
            }
            Ok(found)
        })
    }

    pub fn revoke_all(&self) -> Result<usize> {
        let now = self.now();
        self.update(|data| {
            let mut count = 0;
            for entry in &mut data.sessions {
                if !entry.revoked() {
                    tombstone(entry, now);
                    count += 1;
                }
            }
            Ok(count)
        })
    }

    pub fn rotate(
        &self,
        token: &str,
        csrf: &str,
        origin: &str,
        now: i64,
    ) -> Result<Result<IssuedSession, AuthFailure>> {
        let hash = digest(token);
        self.write(|data| {
            let Some(position) = find_session(&data.sessions, &hash) else {
                return Ok(Err(AuthFailure::Invalid));
            };
            let entry = &mut data.sessions[position];
            if let Err(error) = validate(entry, Some(csrf), Some(origin), false, now) {
                return Ok(Err(error));
            }
            let new_token = random_token()?;
            let new_csrf = random_token()?;
            entry.token_hash = digest(&new_token);
            entry.csrf_hash = digest(&new_csrf);
            touch(&mut entry.device, now);
            Ok(Ok(IssuedSession {
                token: new_token,
                csrf: new_csrf,
                device: Device::from(&entry.device).at(now),
            }))
        })
    }
}

/// Record use: only the idle clock restarts; the absolute lifetime does not move
/// (HS2-4B0KSD). It never moves backward, so a clock step back cannot shorten a session.
fn touch(device: &mut DeviceRecord, now: i64) {
    device.last_used_at = device.last_used_at.max(now);
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).context("generate remote credential")?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn validate(
    entry: &SessionRecord,
    csrf: Option<&str>,
    origin: Option<&str>,
    write: bool,
    now: i64,
) -> Result<Device, AuthFailure> {
    if entry.revoked() {
        return Err(AuthFailure::Revoked);
    }
    if entry.device.expired(now) {
        return Err(AuthFailure::SessionExpired);
    }
    if write && entry.device.role == Role::ReadOnly {
        return Err(AuthFailure::ReadOnly);
    }
    if origin.is_some_and(|value| value != entry.device.origin) || (write && origin.is_none()) {
        return Err(AuthFailure::Invalid);
    }
    if (write || csrf.is_some())
        && !csrf.is_some_and(|value| equal_digest(&entry.csrf_hash, &digest(value)))
    {
        return Err(AuthFailure::Csrf);
    }
    Ok(Device::from(&entry.device))
}

fn valid_origin(origin: &str) -> bool {
    let Some((scheme, host)) = origin.split_once("://") else {
        return false;
    };
    matches!(scheme, "http" | "https") && !host.is_empty() && !host.contains(['/', '?', '#', '@'])
}

fn equal_digest(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |result, (a, b)| result | (a ^ b))
        == 0
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("remote auth catalog has no parent")?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".remote-auth-{}", Ulid::new()));
    let result = (|| -> Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        hotsheet_sync::files::replace(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_is_owner_approved_single_use_and_durable_without_plaintext_tokens() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("remote-auth.json");
        let auth = RemoteAuth::open(&path, "synthetic-server")?;
        let pair = auth
            .request("Synthetic tablet", "http://localhost:4175", 100)?
            .unwrap();
        assert_eq!(
            auth.redeem(&pair.id, &pair.challenge, "http://localhost:4175", 101)?
                .unwrap_err(),
            AuthFailure::Unapproved
        );
        assert_eq!(
            auth.redeem(&pair.id, "wrong", "http://localhost:4175", 102)?
                .unwrap_err(),
            AuthFailure::Invalid
        );
        auth.approve(&pair.id, Role::ReadOnly, Grants::default(), 103)?
            .unwrap();
        let issued = auth
            .redeem(&pair.id, &pair.challenge, "http://localhost:4175", 104)?
            .unwrap();
        assert_eq!(
            auth.redeem(&pair.id, &pair.challenge, "http://localhost:4175", 105)?
                .unwrap_err(),
            AuthFailure::Invalid
        );
        assert_eq!(
            auth.authorize(
                &issued.token,
                None,
                Some("http://localhost:4175"),
                true,
                106
            )?
            .unwrap_err(),
            AuthFailure::ReadOnly
        );
        assert!(
            auth.authorize(&issued.token, None, None, false, 106)?
                .is_ok()
        );
        let file = fs::read_to_string(&path)?;
        assert!(!file.contains(&pair.challenge));
        assert!(!file.contains(&issued.token));
        assert!(!file.contains(&issued.csrf));
        let reopened = RemoteAuth::open(&path, "synthetic-server")?;
        assert_eq!(reopened.server_id(), issued.device.server_id);
        assert!(
            reopened
                .authorize(&issued.token, None, None, false, 107)?
                .is_ok()
        );
        assert!(reopened.revoke(&issued.device.id)?);
        assert_eq!(
            reopened
                .authorize(&issued.token, None, None, false, 108)?
                .unwrap_err(),
            AuthFailure::Revoked
        );
        let file = fs::read_to_string(&path)?;
        assert!(!file.contains(&issued.token));
        Ok(())
    }

    #[test]
    fn expiry_attempts_and_global_request_limit() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let auth = RemoteAuth::open(dir.path().join("remote-auth.json"), "synthetic-server")?;
        let first = auth
            .request("Synthetic device", "http://localhost:4175", 100)?
            .unwrap();
        assert_eq!(
            auth.approve(&first.id, Role::ReadWrite, Grants::default(), 1001)?
                .unwrap_err(),
            AuthFailure::Expired
        );
        let second = auth
            .request("Synthetic device", "http://localhost:4175", 1002)?
            .unwrap();
        for _ in 0..10 {
            assert_eq!(
                auth.redeem(&second.id, &second.challenge, "http://localhost:4175", 1003)?
                    .unwrap_err(),
                AuthFailure::Unapproved
            );
        }
        for _ in 0..5 {
            assert_eq!(
                auth.redeem(&second.id, "wrong", "http://localhost:4175", 1003)?
                    .unwrap_err(),
                AuthFailure::Invalid
            );
        }
        assert_eq!(
            auth.redeem(&second.id, &second.challenge, "http://localhost:4175", 1003)?
                .unwrap_err(),
            AuthFailure::RateLimited
        );
        for _ in 0..9 {
            auth.request("Synthetic device", "http://localhost:4175", 1004)?
                .unwrap();
        }
        assert_eq!(
            auth.request("Synthetic device", "http://localhost:4175", 1004)?
                .unwrap_err(),
            AuthFailure::RateLimited
        );
        Ok(())
    }

    #[test]
    fn fifteen_minute_pairing_carries_admin_and_local_grants_through_redeem_and_reopen()
    -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("remote-auth.json");
        let auth = RemoteAuth::open(&path, "synthetic-server")?;
        let origin = "http://127.0.0.1:4175";
        let pair = auth.request("Synthetic Mac", origin, 100)?.unwrap();
        assert_eq!(pair.expires_at, 100 + 15 * 60);
        // Still approvable one second before the fifteen-minute boundary (HS2-DXM0YN).
        auth.approve(
            &pair.id,
            Role::ReadWrite,
            Grants {
                admin: true,
                local: true,
            },
            100 + 15 * 60 - 1,
        )?
        .unwrap();
        let issued = auth
            .redeem(&pair.id, &pair.challenge, origin, 999)?
            .unwrap();
        assert!(issued.device.admin && issued.device.local);
        let plain = auth.request("Synthetic phone", origin, 1001)?.unwrap();
        auth.approve(&plain.id, Role::ReadOnly, Grants::default(), 1002)?
            .unwrap();
        let phone = auth
            .redeem(&plain.id, &plain.challenge, origin, 1003)?
            .unwrap();
        assert!(!phone.device.admin && !phone.device.local);

        let reopened = RemoteAuth::open(&path, "synthetic-server")?;
        let device = reopened
            .authorize(&issued.token, None, None, false, 1004)?
            .unwrap();
        assert!(device.admin && device.local);
        let changed = reopened
            .change(
                &phone.device.id,
                DeviceChange {
                    role: Some(Role::ReadWrite),
                    admin: Some(true),
                    local: None,
                },
            )?
            .unwrap();
        assert_eq!(
            (changed.role, changed.admin, changed.local),
            (Role::ReadWrite, true, false)
        );
        let demoted = reopened
            .change(
                &issued.device.id,
                DeviceChange {
                    local: Some(false),
                    ..DeviceChange::default()
                },
            )?
            .unwrap();
        assert!(demoted.admin && !demoted.local);
        assert!(
            reopened
                .change("missing", DeviceChange::default())?
                .is_none()
        );
        let again = RemoteAuth::open(&path, "synthetic-server")?;
        let listed = again.list(1005);
        let phone_now = listed
            .iter()
            .find(|entry| entry.id == phone.device.id)
            .unwrap();
        assert!(phone_now.admin && phone_now.role == Role::ReadWrite);
        Ok(())
    }

    #[test]
    fn reject_removes_a_pending_request_and_its_redeem_fails() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let auth = RemoteAuth::open(dir.path().join("remote-auth.json"), "synthetic-server")?;
        let origin = "http://localhost:4175";
        let pair = auth.request("Synthetic tablet", origin, 100)?.unwrap();
        assert!(auth.reject(&pair.id)?);
        assert!(!auth.reject(&pair.id)?);
        assert!(auth.pending(101).is_empty());
        assert_eq!(
            auth.approve(&pair.id, Role::ReadOnly, Grants::default(), 102)?
                .unwrap_err(),
            AuthFailure::Invalid
        );
        assert_eq!(
            auth.redeem(&pair.id, &pair.challenge, origin, 103)?
                .unwrap_err(),
            AuthFailure::Invalid
        );
        Ok(())
    }

    #[test]
    fn a_catalog_written_before_grants_existed_loads_without_them() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("remote-auth.json");
        let token = "synthetic-token";
        let legacy = serde_json::json!({
            "server_id": "synthetic-server",
            "pending": [{
                "id": "P1", "label": "Old request", "origin": "http://localhost:4175",
                "challenge_hash": digest("c"), "created_at": 100, "expires_at": 400,
                "attempts": 0, "approved_role": "read_write"
            }],
            "sessions": [{
                "device": {
                    "id": "D1", "server_id": "synthetic-server", "label": "Old device",
                    "role": "read_write", "origin": "http://localhost:4175",
                    "created_at": 100, "last_used_at": 100, "expires_at": 100000
                },
                "token_hash": digest(token),
                "csrf_hash": digest("csrf")
            }]
        });
        fs::write(&path, serde_json::to_vec(&legacy)?)?;
        let auth = RemoteAuth::open(&path, "synthetic-server")?;
        let device = auth.authorize(token, None, None, false, 101)?.unwrap();
        assert!(!device.admin && !device.local);
        let redeemed = auth
            .redeem("P1", "c", "http://localhost:4175", 102)?
            .unwrap();
        assert!(!redeemed.device.admin && !redeemed.device.local);
        Ok(())
    }

    #[test]
    fn visible_changes_advance_the_cursor_and_bookkeeping_does_not() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let auth = RemoteAuth::open(dir.path().join("remote-auth.json"), "synthetic-server")?;
        let origin = "http://localhost:4175";
        let mut cursor = auth.revision();
        let mut advanced = |auth: &RemoteAuth| {
            let next = auth.revision();
            let moved = next != cursor;
            cursor = next;
            moved
        };
        let pair = auth.request("Synthetic", origin, 100)?.unwrap();
        assert!(advanced(&auth));
        auth.approve(&pair.id, Role::ReadWrite, Grants::default(), 101)?
            .unwrap();
        assert!(advanced(&auth));
        let issued = auth
            .redeem(&pair.id, &pair.challenge, origin, 102)?
            .unwrap();
        assert!(advanced(&auth));
        // Idle-activity persistence and credential rotation are not list changes.
        auth.authorize(&issued.token, None, None, false, 500)?
            .unwrap();
        let rotated = auth
            .rotate(&issued.token, &issued.csrf, origin, 501)?
            .unwrap();
        assert!(!advanced(&auth));
        auth.change(
            &rotated.device.id,
            DeviceChange {
                admin: Some(true),
                ..DeviceChange::default()
            },
        )?;
        assert!(advanced(&auth));
        let other = auth.request("Synthetic", origin, 600)?.unwrap();
        assert!(advanced(&auth));
        auth.reject(&other.id)?;
        assert!(advanced(&auth));
        auth.revoke(&rotated.device.id)?;
        assert!(advanced(&auth));
        Ok(())
    }

    /// Pair one read-write device at `now` and return its session.
    fn issue(auth: &RemoteAuth, origin: &str, now: i64) -> Result<IssuedSession> {
        let pair = auth.request("Synthetic laptop", origin, now)?.unwrap();
        auth.approve(&pair.id, Role::ReadWrite, Grants::default(), now)?
            .unwrap();
        Ok(auth
            .redeem(&pair.id, &pair.challenge, origin, now)?
            .unwrap())
    }

    fn writes(auth: &RemoteAuth) -> u64 {
        *auth.writes().borrow()
    }

    const ORIGIN: &str = "http://localhost:4175";

    #[test]
    fn hs2_4b0ksd_use_keeps_a_device_past_thirty_days_until_the_ninety_day_cap() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("remote-auth.json");
        let start = 1_000_000;
        let mut auth = RemoteAuth::open(&path, "synthetic-server")?;
        let issued = issue(&auth, ORIGIN, start)?;
        assert_eq!(issued.device.renewed_at, start);
        assert_eq!(issued.device.expires_at, start + SESSION_TTL);
        assert_eq!(issued.device.idle_expires_at, start + IDLE_TTL);
        // Use every 12 hours; restart the server every 10 days.
        let mut now = start;
        while now + 12 * 3600 < start + SESSION_TTL {
            now += 12 * 3600;
            if (now - start) % (10 * DAY) == 0 {
                auth = RemoteAuth::open(&path, "synthetic-server")?;
            }
            let device = auth
                .authorize(&issued.token, None, None, false, now)?
                .unwrap_or_else(|error| panic!("{}: {error:?}", now - start));
            assert_eq!(device.last_used_at, now);
            // Use never moves the absolute end.
            assert_eq!(device.expires_at, start + SESSION_TTL);
        }
        assert!(now - start > IDLE_TTL);
        // The cap holds however recently the device was used.
        assert_eq!(
            auth.authorize(&issued.token, None, None, false, start + SESSION_TTL)?
                .unwrap_err(),
            AuthFailure::SessionExpired
        );
        let listed = auth.list(start + SESSION_TTL);
        assert!(listed[0].expired && listed[0].id == issued.device.id);
        Ok(())
    }

    #[test]
    fn hs2_4b0ksd_a_device_idle_past_thirty_days_expires_and_use_cannot_revive_it() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("remote-auth.json");
        let auth = RemoteAuth::open(&path, "synthetic-server")?;
        let issued = issue(&auth, ORIGIN, 100)?;
        // One second short of the idle limit still counts, and restarts the idle clock.
        let last = 100 + IDLE_TTL - 1;
        assert!(
            auth.authorize(&issued.token, None, None, false, last)?
                .is_ok()
        );
        assert!(
            auth.authorize(&issued.token, None, None, false, last + IDLE_TTL - 1)?
                .is_ok()
        );
        let last = last + IDLE_TTL - 1;
        // Durable: a restart, later requests and a rotation all stay expired.
        let reopened = RemoteAuth::open(&path, "synthetic-server")?;
        for at in [last + IDLE_TTL, last + IDLE_TTL + 1, last + 3 * IDLE_TTL] {
            assert_eq!(
                reopened
                    .authorize(&issued.token, None, None, false, at)?
                    .unwrap_err(),
                AuthFailure::SessionExpired
            );
        }
        assert_eq!(
            reopened
                .rotate(&issued.token, &issued.csrf, ORIGIN, last + IDLE_TTL + 2)?
                .unwrap_err(),
            AuthFailure::SessionExpired
        );
        let listed = reopened.list(last + IDLE_TTL);
        assert_eq!(listed.len(), 1);
        assert!(listed[0].expired);
        Ok(())
    }

    #[test]
    fn hs2_4b0ksd_renewal_before_and_after_expiry_keeps_the_device_and_its_credential() -> Result<()>
    {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("remote-auth.json");
        let auth = RemoteAuth::open(&path, "synthetic-server")?;
        let issued = issue(&auth, ORIGIN, 0)?;
        auth.change(
            &issued.device.id,
            DeviceChange {
                admin: Some(true),
                ..DeviceChange::default()
            },
        )?;
        // Before expiry: day 80, while in use.
        for day in [25, 50, 75, 80] {
            auth.authorize(&issued.token, None, None, false, day * DAY)?
                .unwrap();
        }
        let renewed = auth.renew(&issued.device.id, 80 * DAY)?.unwrap();
        assert_eq!(renewed.id, issued.device.id);
        assert_eq!(renewed.renewed_at, 80 * DAY);
        assert_eq!(renewed.expires_at, 80 * DAY + SESSION_TTL);
        assert_eq!(renewed.reminder_day, None);
        assert!(renewed.admin && renewed.role == Role::ReadWrite);
        // The same token and CSRF proof keep working past the original cap.
        assert!(
            auth.authorize(
                &issued.token,
                Some(&issued.csrf),
                Some(ORIGIN),
                true,
                100 * DAY
            )?
            .is_ok()
        );
        // After expiry (idle past 30 days): the device asks, the host renews, it works again.
        let idle = 100 * DAY + IDLE_TTL + 1;
        assert_eq!(
            auth.authorize(&issued.token, None, None, false, idle)?
                .unwrap_err(),
            AuthFailure::SessionExpired
        );
        assert_eq!(
            auth.request_renewal(&issued.token, "wrong", ORIGIN, idle)?
                .unwrap_err(),
            AuthFailure::Csrf
        );
        assert_eq!(
            auth.request_renewal(&issued.token, &issued.csrf, "http://elsewhere:1", idle)?
                .unwrap_err(),
            AuthFailure::Invalid
        );
        let asked = auth
            .request_renewal(&issued.token, &issued.csrf, ORIGIN, idle)?
            .unwrap();
        assert_eq!(asked.renewal_requested_at, Some(idle));
        // Asking again is idempotent and writes nothing.
        let before = writes(&auth);
        let again = auth
            .request_renewal(&issued.token, &issued.csrf, ORIGIN, idle + 5)?
            .unwrap();
        assert_eq!(again.renewal_requested_at, Some(idle));
        assert_eq!(writes(&auth), before);
        let reopened = RemoteAuth::open(&path, "synthetic-server")?;
        let renewed = reopened.renew(&issued.device.id, idle + 60)?.unwrap();
        assert_eq!(renewed.renewal_requested_at, None);
        let device = reopened
            .authorize(&issued.token, None, None, false, idle + 61)?
            .unwrap();
        assert_eq!(device.id, issued.device.id);
        assert!(device.admin);
        assert!(reopened.renew("missing", idle)?.is_none());
        Ok(())
    }

    #[test]
    fn hs2_4b0ksd_reminders_fall_on_the_policy_days() {
        let expected: Vec<i64> = [45, 50, 55, 60].into_iter().chain(65..=90).collect();
        assert_eq!(REMINDER_DAYS.to_vec(), expected);
        let renewed = 7_000;
        assert_eq!(reminder_day(renewed, renewed), None);
        assert_eq!(reminder_day(renewed, renewed + 45 * DAY - 1), None);
        let mut shown = Vec::new();
        for hour in 0..=(92 * 24) {
            let now = renewed + hour * 3600;
            if let Some(day) = reminder_day(renewed, now)
                && shown.last() != Some(&day)
            {
                // Each reminder starts exactly on its day.
                assert_eq!(now, renewed + day * DAY, "day {day}");
                shown.push(day);
            }
        }
        assert_eq!(shown, expected);
        // Between thresholds the latest reached day stays.
        assert_eq!(reminder_day(renewed, renewed + 47 * DAY), Some(45));
        assert_eq!(reminder_day(renewed, renewed + 64 * DAY), Some(60));
        assert_eq!(reminder_day(renewed, renewed + 200 * DAY), Some(90));
    }

    #[test]
    fn hs2_4b0ksd_activity_writes_at_most_once_per_interval() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let auth = RemoteAuth::open(dir.path().join("remote-auth.json"), "synthetic-server")?;
        let issued = issue(&auth, ORIGIN, 1_000)?;
        let before = writes(&auth);
        // A busy page plus a stream rechecking every second for one interval: no write within
        // the interval of the redeem's own activity.
        for second in 0..ACTIVITY_PERSIST_INTERVAL {
            for _ in 0..20 {
                assert!(
                    auth.authorize(&issued.token, None, None, false, 1_000 + second)?
                        .is_ok()
                );
            }
        }
        assert_eq!(writes(&auth), before);
        // Each later interval of continuous use costs exactly one write.
        for second in ACTIVITY_PERSIST_INTERVAL..ACTIVITY_PERSIST_INTERVAL * 11 {
            auth.authorize(&issued.token, None, None, false, 1_000 + second)?
                .unwrap();
        }
        assert_eq!(writes(&auth), before + 10);
        // A failed check never writes.
        let failed = writes(&auth);
        assert!(
            auth.authorize("unknown-token", None, None, false, 1_000_000)?
                .is_err()
        );
        assert!(
            auth.authorize(&issued.token, None, None, false, 1_000 + SESSION_TTL)?
                .is_err()
        );
        assert_eq!(writes(&auth), failed);
        Ok(())
    }

    #[test]
    fn hs2_4b0ksd_revoking_an_active_device_reports_revoked_and_blocks_renewal() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("remote-auth.json");
        let auth = RemoteAuth::open(&path, "synthetic-server")?.with_clock(Arc::new(|| 450));
        let issued = issue(&auth, ORIGIN, 100)?;
        let kept = issue(&auth, ORIGIN, 100)?;
        for at in [200, 300, 400] {
            auth.authorize(&issued.token, None, None, false, at)?
                .unwrap();
        }
        assert!(auth.revoke(&issued.device.id)?);
        assert!(!auth.revoke(&issued.device.id)?);
        for at in [451, 500, 100 + SESSION_TTL] {
            assert_eq!(
                auth.authorize(&issued.token, None, None, false, at)?
                    .unwrap_err(),
                AuthFailure::Revoked
            );
        }
        assert_eq!(
            auth.request_renewal(&issued.token, &issued.csrf, ORIGIN, 500)?
                .unwrap_err(),
            AuthFailure::Revoked
        );
        assert!(auth.renew(&issued.device.id, 500)?.is_none());
        assert!(
            auth.change(&issued.device.id, DeviceChange::default())?
                .is_none()
        );
        let reopened = RemoteAuth::open(&path, "synthetic-server")?;
        assert_eq!(
            reopened
                .authorize(&issued.token, None, None, false, 600)?
                .unwrap_err(),
            AuthFailure::Revoked
        );
        // The tombstone is never listed, and revoking one device leaves another alone.
        let listed = reopened.list(600);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, kept.device.id);
        assert!(
            reopened
                .authorize(&kept.token, None, None, false, 600)?
                .is_ok()
        );
        // An unknown token stays invalid, distinct from revoked.
        assert_eq!(
            reopened
                .authorize("unknown", None, None, false, 600)?
                .unwrap_err(),
            AuthFailure::Invalid
        );
        // A later pairing prunes the tombstone once its lifetime passed again.
        let later = 450 + 2 * SESSION_TTL + 1;
        issue(&reopened, ORIGIN, later)?;
        let file = fs::read_to_string(&path)?;
        assert!(!file.contains(REVOKED_PREFIX));
        Ok(())
    }

    #[test]
    fn hs2_4b0ksd_a_catalog_from_before_the_policy_counts_from_approval() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("remote-auth.json");
        let token = "synthetic-token";
        let legacy = serde_json::json!({
            "server_id": "synthetic-server",
            "pending": [],
            "sessions": [{
                "device": {
                    "id": "D1", "server_id": "synthetic-server", "label": "Old device",
                    "role": "read_write", "origin": ORIGIN,
                    "created_at": 0, "last_used_at": 10 * DAY, "expires_at": 30 * DAY
                },
                "token_hash": digest(token),
                "csrf_hash": digest("csrf")
            }]
        });
        fs::write(&path, serde_json::to_vec(&legacy)?)?;
        let auth = RemoteAuth::open(&path, "synthetic-server")?;
        // Past the old 30-day end but inside the new 90-day lifetime and idle window.
        let device = auth.authorize(token, None, None, false, 35 * DAY)?.unwrap();
        assert_eq!((device.renewed_at, device.expires_at), (0, SESSION_TTL));
        Ok(())
    }

    #[test]
    fn hs2_4b0ksd_the_manual_clock_drives_every_reader() {
        let clock = ManualClock::new(5);
        let read = clock.clock();
        assert_eq!(read(), 5);
        assert_eq!(clock.advance(10), 15);
        clock.set(40);
        assert_eq!((read(), clock.now()), (40, 40));
        assert!(wall_clock() > 1_600_000_000);
    }

    #[tokio::test]
    async fn a_long_poll_wakes_on_a_change_and_returns_at_once_for_a_stale_cursor() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let auth = RemoteAuth::open(dir.path().join("remote-auth.json"), "synthetic-server")?;
        let start = auth.revision();
        // A cursor from before a restart differs, so the wait ends immediately.
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            auth.changed_since(start.wrapping_sub(7), std::time::Duration::from_secs(60)),
        )
        .await?;
        let waiter = auth.clone();
        let waiting = tokio::spawn(async move {
            waiter
                .changed_since(start, std::time::Duration::from_secs(60))
                .await;
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        auth.request("Synthetic", "http://localhost:4175", 100)?
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), waiting).await??;
        // An unchanged cursor waits out its limit.
        let unchanged = auth.revision();
        let started = std::time::Instant::now();
        auth.changed_since(unchanged, std::time::Duration::from_millis(50))
            .await;
        assert!(started.elapsed() >= std::time::Duration::from_millis(50));
        Ok(())
    }
}
