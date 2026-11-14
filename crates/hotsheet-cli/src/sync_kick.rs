//! Ask the running host to sync a store after a headless write (HS2-D9ZGAE,
//! `docs/02-ticket-storage.md` §2.12). A CLI write commits locally and starts a best-effort
//! `git push`, which cannot fetch or rebase: once the remote moved on, the commits stay
//! local. After a command, the CLI sends `POST /sync/kick` for every store it committed to,
//! so the host's sync loop fetches, integrates, and pushes it promptly. When no host
//! accepts, the CLI says the commits may be local only and names `hotsheet-cli sync`.
//!
//! The host comes from [`discover_local_host`] under `HOTSHEET_HOME`, so a test or an
//! isolated home never reaches the user's real host.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::external_launch::{HostScope, discover_local_host};

/// How long the CLI waits for the host to answer a kick. The host answers at once; the
/// sync runs on its background loop.
const KICK_TIMEOUT: Duration = Duration::from_secs(5);

/// The host's answer to one kick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KickOutcome {
    /// The host will sync the store promptly.
    Accepted,
    /// No live host was found.
    NoHost,
    /// A host answered but did not accept the store; the reason is the host's message.
    Refused(String),
}

/// Ask the running host to sync the store at `store` once.
pub fn request_host_sync(home: &Path, store: &Path) -> KickOutcome {
    let Ok(server) = discover_local_host(store, home, HostScope::Project) else {
        return KickOutcome::NoHost;
    };
    let store = store.canonicalize().unwrap_or_else(|_| store.to_path_buf());
    let endpoint = format!("{}/sync/kick", server.url.trim_end_matches('/'));
    let body = serde_json::json!({ "store": store }).to_string();
    match ureq::post(&endpoint)
        .set("X-Hotsheet-Secret", &server.secret)
        .set("Content-Type", "application/json")
        .timeout(KICK_TIMEOUT)
        .send_string(&body)
    {
        Ok(_) => KickOutcome::Accepted,
        Err(ureq::Error::Status(status, response)) => {
            let text = response.into_string().unwrap_or_default();
            let message = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|body| body["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("HTTP {status}"));
            KickOutcome::Refused(message)
        }
        Err(error) => KickOutcome::Refused(error.to_string()),
    }
}

/// The stderr line for a store no host accepted, naming the recovery command.
pub fn local_only_message(store: &Path, outcome: &KickOutcome) -> Option<String> {
    let reason = match outcome {
        KickOutcome::Accepted => return None,
        KickOutcome::NoHost => "no Hot Sheet host is running".to_string(),
        KickOutcome::Refused(message) => {
            format!("the running Hot Sheet host did not accept a sync request: {message}")
        }
    };
    Some(format!(
        "note: the commits in {} may be local only ({reason}). Run `hotsheet-cli -C {} sync` \
         to publish them.",
        store.display(),
        store.display()
    ))
}

/// Kick the host for every store this process committed to with a headless push and
/// return the message for each store no host accepted. Each store is kicked once.
pub fn kick_after_writes(home: &Path, stores: Vec<PathBuf>) -> Vec<String> {
    stores
        .iter()
        .filter_map(|store| local_only_message(store, &request_host_sync(home, store)))
        .collect()
}

/// [`kick_after_writes`] for the stores this process recorded since the last call, with
/// each message printed on stderr. Never fails the command: the write is already saved.
pub fn kick_recorded_writes(home: &Path) {
    let stores = hotsheet_ticketing::take_headless_published_stores();
    if stores.is_empty() {
        return;
    }
    for message in kick_after_writes(home, stores) {
        eprintln!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HS2-D9ZGAE: with no host running, the CLI says the commits may be local only and
    /// names the recovery command; an accepted kick prints nothing.
    #[test]
    fn hs2_d9zgae_no_host_reports_local_only_commits_and_names_sync() {
        let home = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        assert_eq!(
            request_host_sync(home.path(), store.path()),
            KickOutcome::NoHost
        );
        let messages = kick_after_writes(home.path(), vec![store.path().to_path_buf()]);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("may be local only"), "{}", messages[0]);
        assert!(messages[0].contains("no Hot Sheet host is running"));
        assert!(
            messages[0].contains(&format!("hotsheet-cli -C {} sync", store.path().display())),
            "{}",
            messages[0]
        );
        assert_eq!(
            local_only_message(store.path(), &KickOutcome::Accepted),
            None
        );
        let refused =
            local_only_message(store.path(), &KickOutcome::Refused("not registered".into()))
                .unwrap();
        assert!(refused.contains("did not accept a sync request: not registered"));
        assert!(kick_after_writes(home.path(), Vec::new()).is_empty());
    }

    /// A stale instance record (its process is gone) is no host, never a hang.
    #[cfg(unix)]
    #[test]
    fn hs2_d9zgae_a_stale_host_record_is_no_host() {
        let home = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let record = crate::external_launch::instance_path(home.path(), store.path());
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(
            &record,
            serde_json::json!({
                "pid": u32::MAX - 1,
                "url": "http://127.0.0.1:9",
                "secret": "secret",
                "store_path": store.path(),
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            request_host_sync(home.path(), store.path()),
            KickOutcome::NoHost
        );
    }
}
