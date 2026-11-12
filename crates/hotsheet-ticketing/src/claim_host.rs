//! The Hot Sheet host that writes a claim (HS2-DQJPW0).
//!
//! Every server, CLI and MCP process on one machine shares a machine home (`HOTSHEET_HOME`). Its
//! `host-id` file holds one random, opaque ULID created on first use. Claims record it, so a
//! server that sees a claim through a synced Git ticket store can tell whether the claim came from
//! itself or from a separate Hot Sheet host, whose worker names are local to that machine. A
//! browser or phone attached to a server is not a host: its claims are made by that server.
//!
//! The id identifies nothing beyond the machine home: it is not a machine, user, or account id,
//! and it is not a secret.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use hotsheet_model::Ticket;

/// The machine-home file that holds the host id.
pub const HOST_ID_FILE: &str = "host-id";

/// Where a claim came from, relative to the reading host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimOrigin {
    /// A worker of this Hot Sheet host holds the claim.
    Local,
    /// A separate Hot Sheet host's worker holds the claim; its worker names are local to it.
    Remote,
}

/// This machine home's host id, created on first use. `None` when the home cannot hold it (for
/// example a read-only home): the claim is then simply recorded without a host. The id is
/// cached per machine home, so list conversions read the file once.
pub fn local_host_id() -> Option<String> {
    use std::sync::Mutex;
    static CACHE: Mutex<Option<(std::path::PathBuf, String)>> = Mutex::new(None);
    let home = crate::settings::hotsheet_home();
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((cached_home, id)) = cache.as_ref()
        && *cached_home == home
    {
        return Some(id.clone());
    }
    let id = host_id_in(&home)?;
    *cache = Some((home, id.clone()));
    Some(id)
}

/// The host id stored in `home`, creating it when absent. Two racing first writers settle on
/// whichever file was created first.
pub fn host_id_in(home: &Path) -> Option<String> {
    let path = home.join(HOST_ID_FILE);
    if let Some(id) = read(&path) {
        return Some(id);
    }
    std::fs::create_dir_all(home).ok()?;
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            let id = hotsheet_model::Ulid::new().to_string();
            file.write_all(id.as_bytes()).ok()?;
            file.sync_all().ok()?;
            Some(id)
        }
        Err(_) => read(&path),
    }
}

fn read(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let id = text.trim();
    (!id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then(|| id.to_owned())
}

/// The origin of `ticket`'s claim as seen by host `local`, or `None` when no claim is held or the
/// claim was written before hosts were recorded (its origin is unknown).
pub fn claim_origin(ticket: &Ticket, local: Option<&str>) -> Option<ClaimOrigin> {
    origin(
        ticket.claimed_by.as_deref(),
        ticket.claim_host.as_deref(),
        local,
    )
}

/// [`claim_origin`] over the indexed fields of a ticket row.
pub fn origin(
    claimed_by: Option<&str>,
    claim_host: Option<&str>,
    local: Option<&str>,
) -> Option<ClaimOrigin> {
    claimed_by?;
    let host = claim_host?;
    Some(if Some(host) == local {
        ClaimOrigin::Local
    } else {
        ClaimOrigin::Remote
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_dqjpw0_host_id_is_created_once_and_reused() {
        let home = tempfile::tempdir().unwrap();
        let first = host_id_in(home.path()).unwrap();
        assert_eq!(host_id_in(home.path()).as_deref(), Some(first.as_str()));
        assert_eq!(first.len(), 26);
        let other = tempfile::tempdir().unwrap();
        assert_ne!(host_id_in(other.path()).unwrap(), first);
        // An unreadable value is replaced only if the file is absent; garbage reads as none.
        std::fs::write(other.path().join(HOST_ID_FILE), "not valid!").unwrap();
        assert_eq!(host_id_in(other.path()), None);
    }

    #[test]
    fn hs2_dqjpw0_origin_is_local_remote_or_unknown() {
        let local = Some("HOSTA");
        assert_eq!(origin(None, Some("HOSTA"), local), None, "no live claim");
        assert_eq!(
            origin(Some("w"), None, local),
            None,
            "legacy claim: unknown"
        );
        assert_eq!(
            origin(Some("w"), Some("HOSTA"), local),
            Some(ClaimOrigin::Local)
        );
        assert_eq!(
            origin(Some("w"), Some("HOSTB"), local),
            Some(ClaimOrigin::Remote)
        );
        // A reader that cannot know its own host sees every recorded host as remote.
        assert_eq!(
            origin(Some("w"), Some("HOSTA"), None),
            Some(ClaimOrigin::Remote)
        );
    }
}
