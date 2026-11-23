//! HS2-YDDPN4: merge the per-store session catalogs into the machine catalog.
//!
//! Before HS2-YDDPN4 a server kept its paired devices in `remote-auth/<store-id>.json`, keyed by
//! its primary store, so a server started on another primary store knew none of them. The machine
//! catalog's first open merges every such file found beside it, under the catalog's lock:
//!
//! - Devices merge by id. A device present on one side only is copied.
//! - When both sides hold the same device with different records, a revocation on either side
//!   wins, so a revoked device stays revoked. Otherwise the record with the newest renewal wins
//!   and keeps the later idle activity of the two. Each such conflict records a warning.
//! - Unexpired pending pairings are copied; the request rate limiter is not.
//! - No session is dropped: nothing is pruned during the merge.
//! - A source that cannot be read or parsed is skipped with a warning and tried again at the next
//!   open, so a repaired file still merges.
//! - Each merged source is recorded with its SHA-256 digest. An unchanged source is never merged
//!   again, so repeated opens change nothing; a source an older server rewrote merges again.
//! - The per-store files stay where they are, so an older server can still read its own.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Data, SessionRecord};

/// One per-store catalog already merged, recorded in the machine catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MigratedSource {
    /// The source's file name inside the catalog directory.
    file: String,
    /// The SHA-256 of the source bytes that were merged.
    sha256: String,
    /// When the merge happened, in Unix seconds.
    at: i64,
    /// The conflicts and skips this source produced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

/// What one open merged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// The per-store catalog files merged by this open, by file name.
    pub merged: Vec<String>,
    /// Every warning, prefixed with the source's file name.
    pub warnings: Vec<String>,
    /// The machine catalog changed and must be written.
    pub(super) changed: bool,
}

/// A per-store catalog's name: the 16 lowercase hex digits of a store id, then `.json`.
fn is_store_catalog(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.strip_suffix(".json").is_some_and(|stem| {
        stem.len() == 16
            && stem
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Merge every per-store catalog in `dir` into `target`, at `now`. `catalog` is the machine
/// catalog's own path, which is never treated as a source.
pub(super) fn migrate(target: &mut Data, dir: &Path, catalog: &Path, now: i64) -> MigrationReport {
    let mut report = MigrationReport::default();
    let Ok(entries) = fs::read_dir(dir) else {
        return report;
    };
    let mut sources: Vec<_> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path != catalog && is_store_catalog(path))
        .collect();
    sources.sort();
    for source in sources {
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = match fs::read(&source) {
            Ok(bytes) => bytes,
            Err(error) => {
                report
                    .warnings
                    .push(format!("{name}: skipped, unreadable ({error})"));
                continue;
            }
        };
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        if target
            .migrated
            .iter()
            .any(|done| done.file == name && done.sha256 == sha256)
        {
            continue;
        }
        let mut warnings = Vec::new();
        match serde_json::from_slice::<Data>(&bytes) {
            Err(error) => {
                report.warnings.push(format!(
                    "{name}: skipped, not a valid remote auth catalog ({error})"
                ));
                continue;
            }
            Ok(data) if data.server_id != target.server_id => {
                warnings.push("skipped, it belongs to another server identity".to_owned());
            }
            Ok(data) => merge(target, data, now, &mut warnings),
        }
        report
            .warnings
            .extend(warnings.iter().map(|warning| format!("{name}: {warning}")));
        target.migrated.retain(|done| done.file != name);
        target.migrated.push(MigratedSource {
            file: name.clone(),
            sha256,
            at: now,
            warnings,
        });
        report.merged.push(name);
        report.changed = true;
    }
    report
}

/// Merge one source catalog's devices and live pairings into `target`.
fn merge(target: &mut Data, source: Data, now: i64, warnings: &mut Vec<String>) {
    for incoming in source.sessions {
        let Some(existing) = target
            .sessions
            .iter_mut()
            .find(|entry| entry.device.id == incoming.device.id)
        else {
            target.sessions.push(incoming);
            continue;
        };
        if *existing == incoming {
            continue;
        }
        let id = incoming.device.id.clone();
        match (existing.revoked(), incoming.revoked()) {
            (true, true) => {
                if incoming.device.renewed() > existing.device.renewed() {
                    *existing = incoming;
                }
            }
            (true, false) => warnings.push(format!(
                "device {id} is revoked in the machine catalog and live in this one; kept revoked"
            )),
            (false, true) => {
                *existing = incoming;
                warnings.push(format!(
                    "device {id} is live in the machine catalog and revoked in this one; kept revoked"
                ));
            }
            (false, false) => {
                let last_used_at = existing
                    .device
                    .last_used_at
                    .max(incoming.device.last_used_at);
                if newer(&incoming, existing) {
                    *existing = incoming;
                }
                existing.device.last_used_at = last_used_at;
                warnings.push(format!(
                    "device {id} has conflicting records; kept the newest renewal"
                ));
            }
        }
    }
    for pending in source.pending {
        if pending.expires_at > now && !target.pending.iter().any(|entry| entry.id == pending.id) {
            target.pending.push(pending);
        }
    }
}

/// Whether `candidate` was renewed later than `current`, or, renewed together, used later.
fn newer(candidate: &SessionRecord, current: &SessionRecord) -> bool {
    (candidate.device.renewed(), candidate.device.last_used_at)
        > (current.device.renewed(), current.device.last_used_at)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anyhow::Result;

    use super::super::{
        AuthFailure, CATALOG_DIR, Grants, IssuedSession, RemoteAuth, Role, machine_catalog_path,
        wall_clock,
    };

    const SERVER: &str = "synthetic-server";
    const ORIGIN: &str = "http://localhost:4175";
    const STORE_A: &str = "aaaaaaaaaaaaaaaa.json";
    const STORE_B: &str = "bbbbbbbbbbbbbbbb.json";
    const STORE_C: &str = "cccccccccccccccc.json";

    fn legacy(home: &std::path::Path, name: &str) -> PathBuf {
        home.join(CATALOG_DIR).join(name)
    }

    /// Pair one device through the real request, approve and redeem transitions.
    fn pair(auth: &RemoteAuth, label: &str, now: i64) -> Result<IssuedSession> {
        let pairing = auth.request(label, ORIGIN, now)?.unwrap();
        auth.approve(&pairing.id, Role::ReadWrite, Grants::default(), now)?
            .unwrap();
        Ok(auth
            .redeem(&pairing.id, &pairing.challenge, ORIGIN, now)?
            .unwrap())
    }

    fn works(auth: &RemoteAuth, session: &IssuedSession, now: i64) -> Result<bool> {
        Ok(auth
            .authorize(&session.token, None, None, false, now)?
            .is_ok())
    }

    #[test]
    fn two_store_catalogs_merge_into_the_machine_catalog_and_stay_in_place() -> Result<()> {
        let home = tempfile::tempdir()?;
        let now = wall_clock();
        let tablet = pair(
            &RemoteAuth::open(legacy(home.path(), STORE_A), SERVER)?,
            "Synthetic tablet",
            now,
        )?;
        let source_b = RemoteAuth::open(legacy(home.path(), STORE_B), SERVER)?;
        let phone = pair(&source_b, "Synthetic phone", now)?;
        // A pairing still waiting for approval carries over too.
        let waiting = source_b.request("Synthetic laptop", ORIGIN, now)?.unwrap();
        let before_a = std::fs::read(legacy(home.path(), STORE_A))?;

        let machine = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        assert_eq!(machine.migration().merged, vec![STORE_A, STORE_B]);
        assert!(machine.migration().warnings.is_empty());
        assert!(works(&machine, &tablet, now + 1)?);
        assert!(works(&machine, &phone, now + 1)?);
        machine
            .approve(&waiting.id, Role::ReadOnly, Grants::default(), now + 2)?
            .unwrap();
        let laptop = machine
            .redeem(&waiting.id, &waiting.challenge, ORIGIN, now + 3)?
            .unwrap();
        assert!(works(&machine, &laptop, now + 4)?);
        assert_eq!(machine.list(now + 4).len(), 3);
        assert_eq!(std::fs::read(legacy(home.path(), STORE_A))?, before_a);
        Ok(())
    }

    #[test]
    fn repeated_migration_is_a_no_op_and_a_redeemed_pairing_never_returns() -> Result<()> {
        let home = tempfile::tempdir()?;
        let now = wall_clock();
        let source = RemoteAuth::open(legacy(home.path(), STORE_A), SERVER)?;
        pair(&source, "Synthetic tablet", now)?;
        let waiting = source.request("Synthetic phone", ORIGIN, now)?.unwrap();
        drop(source);

        let first = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        first
            .approve(&waiting.id, Role::ReadOnly, Grants::default(), now + 1)?
            .unwrap();
        first
            .redeem(&waiting.id, &waiting.challenge, ORIGIN, now + 2)?
            .unwrap();
        drop(first);
        let catalog = std::fs::read(machine_catalog_path(home.path()))?;

        for _ in 0..2 {
            let again = RemoteAuth::open_machine_at(home.path(), SERVER, now + 3)?;
            assert!(again.migration().merged.is_empty());
            assert!(again.migration().warnings.is_empty());
            assert!(again.pending(now + 3).is_empty());
            assert_eq!(again.list(now + 3).len(), 2);
            assert_eq!(std::fs::read(machine_catalog_path(home.path()))?, catalog);
        }
        Ok(())
    }

    #[test]
    fn a_corrupt_source_is_skipped_with_a_warning_and_merges_once_repaired() -> Result<()> {
        let home = tempfile::tempdir()?;
        let now = wall_clock();
        let tablet = pair(
            &RemoteAuth::open(legacy(home.path(), STORE_A), SERVER)?,
            "Synthetic tablet",
            now,
        )?;
        std::fs::write(legacy(home.path(), STORE_C), b"{ not json")?;
        // Files that are not per-store catalogs are never sources.
        std::fs::write(home.path().join(CATALOG_DIR).join("notes.json"), b"{}")?;

        let machine = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        assert_eq!(machine.migration().merged, vec![STORE_A]);
        assert_eq!(machine.migration().warnings.len(), 1);
        assert!(
            machine.migration().warnings[0].starts_with(&format!("{STORE_C}: skipped")),
            "{:?}",
            machine.migration().warnings
        );
        assert!(works(&machine, &tablet, now)?);
        drop(machine);

        // Not recorded as merged: the next open warns again, and a repaired file merges.
        let again = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        assert_eq!(again.migration().warnings.len(), 1);
        drop(again);
        std::fs::remove_file(legacy(home.path(), STORE_C))?;
        let phone = pair(
            &RemoteAuth::open(legacy(home.path(), STORE_C), SERVER)?,
            "Synthetic phone",
            now,
        )?;
        let repaired = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        assert_eq!(repaired.migration().merged, vec![STORE_C]);
        assert!(works(&repaired, &tablet, now)?);
        assert!(works(&repaired, &phone, now)?);
        Ok(())
    }

    #[test]
    fn a_source_of_another_server_identity_is_skipped_once() -> Result<()> {
        let home = tempfile::tempdir()?;
        let now = wall_clock();
        let foreign = pair(
            &RemoteAuth::open(legacy(home.path(), STORE_A), "synthetic-other-server")?,
            "Synthetic tablet",
            now,
        )?;
        let machine = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        assert_eq!(
            machine.migration().warnings,
            vec![format!(
                "{STORE_A}: skipped, it belongs to another server identity"
            )]
        );
        assert_eq!(
            machine
                .authorize(&foreign.token, None, None, false, now)?
                .unwrap_err(),
            AuthFailure::Invalid
        );
        drop(machine);
        let again = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        assert!(again.migration().warnings.is_empty());
        Ok(())
    }

    #[test]
    fn revocation_stays_revoked_through_a_merge_from_either_side() -> Result<()> {
        let home = tempfile::tempdir()?;
        let now = wall_clock();
        let source = RemoteAuth::open(legacy(home.path(), STORE_A), SERVER)?;
        let tablet = pair(&source, "Synthetic tablet", now)?;
        let phone = pair(&source, "Synthetic phone", now)?;
        drop(source);
        // A copied catalog (the same devices under a second store id) where the phone was
        // revoked: the revocation wins over the live copy.
        std::fs::copy(legacy(home.path(), STORE_A), legacy(home.path(), STORE_B))?;
        assert!(RemoteAuth::open(legacy(home.path(), STORE_B), SERVER)?.revoke(&phone.device.id)?);

        let machine = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        assert_eq!(
            machine
                .authorize(&phone.token, None, None, false, now)?
                .unwrap_err(),
            AuthFailure::Revoked
        );
        assert!(
            machine
                .migration()
                .warnings
                .iter()
                .any(|warning| warning.contains(&format!("device {} is live", phone.device.id))),
            "{:?}",
            machine.migration().warnings
        );
        // Revoke the tablet in the machine catalog, then let an older server rewrite its
        // per-store file (a later use): the re-merge keeps the tablet revoked.
        assert!(machine.revoke(&tablet.device.id)?);
        drop(machine);
        let older = RemoteAuth::open(legacy(home.path(), STORE_A), SERVER)?;
        assert!(works(
            &older,
            &tablet,
            now + 2 * super::super::ACTIVITY_PERSIST_INTERVAL
        )?);
        drop(older);
        let reopened = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        assert_eq!(reopened.migration().merged, vec![STORE_A]);
        assert!(
            reopened.migration().warnings.iter().any(|warning| warning
                .contains(&format!("device {} is revoked", tablet.device.id))),
            "{:?}",
            reopened.migration().warnings
        );
        for session in [&tablet, &phone] {
            assert_eq!(
                reopened
                    .authorize(&session.token, None, None, false, now + 3)?
                    .unwrap_err(),
                AuthFailure::Revoked
            );
        }
        assert!(reopened.list(now + 3).is_empty());
        Ok(())
    }

    #[test]
    fn a_conflict_keeps_the_newest_renewal_and_records_a_warning() -> Result<()> {
        let home = tempfile::tempdir()?;
        let now = wall_clock();
        let tablet = pair(
            &RemoteAuth::open(legacy(home.path(), STORE_A), SERVER)?,
            "Synthetic tablet",
            now,
        )?;
        std::fs::copy(legacy(home.path(), STORE_A), legacy(home.path(), STORE_B))?;
        let renewed_at = now + 10;
        RemoteAuth::open(legacy(home.path(), STORE_B), SERVER)?
            .renew(&tablet.device.id, renewed_at)?
            .unwrap();

        let machine = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        let devices = machine.list(now + 11);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].renewed_at, renewed_at);
        assert!(works(&machine, &tablet, now + 11)?);
        assert_eq!(
            machine.migration().warnings,
            vec![format!(
                "{STORE_B}: device {} has conflicting records; kept the newest renewal",
                tablet.device.id
            )]
        );
        // The warning is durable in the machine catalog, beside the merged source.
        let catalog = std::fs::read_to_string(machine_catalog_path(home.path()))?;
        assert!(catalog.contains("kept the newest renewal"));
        assert!(!catalog.contains(&tablet.token));
        Ok(())
    }

    /// A catalog file replaced by an older copy never moves a live device's idle clock back.
    #[test]
    fn a_reload_never_rolls_back_a_newer_idle_clock() -> Result<()> {
        let home = tempfile::tempdir()?;
        let now = wall_clock();
        let auth = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        let tablet = pair(&auth, "Synthetic tablet", now)?;
        let older = std::fs::read(machine_catalog_path(home.path()))?;
        let later = now + 10 * super::super::ACTIVITY_PERSIST_INTERVAL;
        assert!(works(&auth, &tablet, later)?);
        std::fs::write(machine_catalog_path(home.path()), &older)?;
        assert_eq!(auth.list(later)[0].last_used_at, later);
        // A write applied on top of the older file keeps the newer use too.
        let change = super::super::DeviceChange {
            role: Some(Role::ReadOnly),
            ..Default::default()
        };
        auth.change(&tablet.device.id, change)?.unwrap();
        let reopened = RemoteAuth::open_machine_at(home.path(), SERVER, later)?;
        assert_eq!(reopened.list(later)[0].last_used_at, later);
        Ok(())
    }

    /// Two servers on different primary stores share the machine catalog: neither loses the
    /// other's pairing, and a revocation in one holds in the other at its next request.
    #[test]
    fn two_processes_sharing_the_catalog_see_each_others_changes() -> Result<()> {
        let home = tempfile::tempdir()?;
        let now = wall_clock();
        let first = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        let second = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        let tablet = pair(&first, "Synthetic tablet", now)?;
        let phone = pair(&second, "Synthetic phone", now)?;
        assert!(works(&second, &tablet, now)?);
        assert!(works(&first, &phone, now)?);
        let revision = first.revision();
        assert!(second.revoke(&tablet.device.id)?);
        assert_eq!(
            first
                .authorize(&tablet.token, None, None, false, now)?
                .unwrap_err(),
            AuthFailure::Revoked
        );
        assert_ne!(
            first.revision(),
            revision,
            "a reload wakes device-list waiters"
        );
        let reopened = RemoteAuth::open_machine_at(home.path(), SERVER, now)?;
        assert_eq!(reopened.list(now).len(), 1);
        assert!(works(&reopened, &phone, now)?);
        Ok(())
    }
}
