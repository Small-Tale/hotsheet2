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

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

/// HS2-DXM0YN: long enough to walk to the host and approve.
pub const PAIR_TTL: i64 = 15 * 60;
const SESSION_TTL: i64 = 30 * 24 * 60 * 60;
const IDLE_TTL: i64 = 24 * 60 * 60;
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
    pub expires_at: i64,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionRecord {
    device: DeviceRecord,
    token_hash: String,
    csrf_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    expires_at: i64,
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
            expires_at: value.expires_at,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    server_id: String,
    pending: Vec<PendingRecord>,
    sessions: Vec<SessionRecord>,
    #[serde(default)]
    recent_requests: Vec<i64>,
}

impl Data {
    fn new(server_id: &str) -> Self {
        Self {
            server_id: server_id.to_owned(),
            pending: Vec::new(),
            sessions: Vec::new(),
            recent_requests: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct RemoteAuth {
    path: PathBuf,
    data: Arc<Mutex<Data>>,
    /// In-memory change cursor for device-list long polls. It starts from the clock so a
    /// restarted server never repeats a cursor a client already holds.
    revision: Arc<tokio::sync::watch::Sender<u64>>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AuthFailure {
    Invalid,
    Expired,
    RateLimited,
    Unapproved,
    ReadOnly,
    Csrf,
}

impl RemoteAuth {
    pub fn open(path: impl Into<PathBuf>, server_id: &str) -> Result<Self> {
        let path = path.into();
        let (data, fresh) = match fs::read(&path) {
            Ok(bytes) => (
                serde_json::from_slice(&bytes).context("invalid remote auth catalog")?,
                false,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (Data::new(server_id), true)
            }
            Err(error) => return Err(error).context("read remote auth catalog"),
        };
        if data.server_id != server_id {
            anyhow::bail!("remote auth catalog belongs to another server identity");
        }
        if fresh {
            write_private_atomic(&path, &serde_json::to_vec(&data)?)?;
        }
        let start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        Ok(Self {
            path,
            data: Arc::new(Mutex::new(data)),
            revision: Arc::new(tokio::sync::watch::Sender::new(start)),
        })
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
    fn write<T>(&self, change: impl FnOnce(&mut Data) -> Result<T>) -> Result<T> {
        let mut guard = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut next = guard.clone();
        let result = change(&mut next)?;
        write_private_atomic(&self.path, &serde_json::to_vec(&next)?)?;
        *guard = next;
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
        self.data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
            let Some(entry) = data.sessions.iter_mut().find(|entry| entry.device.id == id) else {
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
            };
            data.sessions.push(SessionRecord {
                device: device.clone(),
                token_hash: digest(&token),
                csrf_hash: digest(&csrf),
            });
            Ok(Ok(IssuedSession {
                token,
                csrf,
                device: Device::from(&device),
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
        // Avoid a disk write for every API read; persist idle activity at most once per
        // minute. The inner recheck still makes concurrent revocation authoritative.
        let hash = digest(token);
        {
            let guard = self
                .data
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(entry) = guard
                .sessions
                .iter()
                .find(|entry| equal_digest(&entry.token_hash, &hash))
            else {
                return Ok(Err(AuthFailure::Invalid));
            };
            let result = validate(entry, csrf, origin, write, now);
            if result.is_err() || entry.device.last_used_at > now - 60 {
                return Ok(result);
            }
        }
        self.write(|data| {
            let Some(entry) = data
                .sessions
                .iter_mut()
                .find(|entry| equal_digest(&entry.token_hash, &hash))
            else {
                return Ok(Err(AuthFailure::Invalid));
            };
            if let Err(error) = validate(entry, csrf, origin, write, now) {
                return Ok(Err(error));
            }
            entry.device.last_used_at = now;
            Ok(Ok(Device::from(&entry.device)))
        })
    }

    pub fn list(&self, now: i64) -> Vec<Device> {
        self.data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .iter()
            .filter(|entry| {
                entry.device.expires_at > now && entry.device.last_used_at + IDLE_TTL > now
            })
            .map(|entry| Device::from(&entry.device))
            .collect()
    }

    pub fn revoke(&self, id: &str) -> Result<bool> {
        self.update(|data| {
            let before = data.sessions.len();
            data.sessions.retain(|entry| entry.device.id != id);
            Ok(before != data.sessions.len())
        })
    }

    pub fn revoke_all(&self) -> Result<usize> {
        self.update(|data| {
            let count = data.sessions.len();
            data.sessions.clear();
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
            let Some(entry) = data
                .sessions
                .iter_mut()
                .find(|entry| equal_digest(&entry.token_hash, &hash))
            else {
                return Ok(Err(AuthFailure::Invalid));
            };
            if let Err(error) = validate(entry, Some(csrf), Some(origin), false, now) {
                return Ok(Err(error));
            }
            let new_token = random_token()?;
            let new_csrf = random_token()?;
            entry.token_hash = digest(&new_token);
            entry.csrf_hash = digest(&new_csrf);
            entry.device.last_used_at = now;
            Ok(Ok(IssuedSession {
                token: new_token,
                csrf: new_csrf,
                device: Device::from(&entry.device),
            }))
        })
    }
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
    if entry.device.expires_at <= now || entry.device.last_used_at + IDLE_TTL <= now {
        return Err(AuthFailure::Expired);
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
        fs::rename(&temp, path)?;
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
        let reopened = RemoteAuth::open(path, "synthetic-server")?;
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
            AuthFailure::Invalid
        );
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
