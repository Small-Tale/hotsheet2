//! Pruning index files left behind by older schema generations (HS2-Y0PAEM).
//!
//! Index files are named `<store-key>.v<schema>.sqlite` ([`super::index_file_name`]), so a
//! schema bump leaves the previous generation's file behind, and builds before HS2-8ZM4PT
//! left an unversioned `<store-key>.sqlite`. Those files are inert caches, but they
//! accumulate. [`prune_stale_index_files`] removes them **only when no live process holds
//! them open**: an older binary that is still running keeps using its own generation's
//! file, and deleting it under that process would turn its cache into an unlinked file.
//!
//! "Held open" is decided by SQLite's own locking, the same probe SQLite uses on close to
//! decide whether it is the last connection: every open WAL-mode connection keeps a shared
//! lock on the database file, so a non-blocking attempt to take an exclusive lock fails
//! while any process (or any other connection in this process) has the file open. Only
//! when that exclusive lock is ours is the file unlinked, and it is unlinked while the lock
//! is still held so no other opener can slip in between the probe and the removal.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use super::{SCHEMA_VERSION, is_corruption, sidecar};

/// What [`prune_stale_index_files`] did, file by file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PruneReport {
    /// Stale database files removed (with their `-wal`/`-shm`/`-journal` sidecars).
    pub removed: Vec<PathBuf>,
    /// Stale database files kept because a live connection holds them open (or they could
    /// not be probed, e.g. no write permission); a later prune retries them.
    pub in_use: Vec<PathBuf>,
}

impl PruneReport {
    /// A one-line human summary for startup / CLI logs, or `None` when nothing was stale.
    pub fn summary(&self) -> Option<String> {
        match (self.removed.len(), self.in_use.len()) {
            (0, 0) => None,
            (removed, 0) => Some(format!(
                "pruned {removed} index file(s) from older schema generations"
            )),
            (removed, in_use) => Some(format!(
                "pruned {removed} index file(s) from older schema generations; kept {in_use} \
                 still held open by a running process"
            )),
        }
    }
}

/// The schema generation an index file name belongs to, if it is an index database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Generation {
    /// `<key>.sqlite`, written before index files were schema-scoped (HS2-8ZM4PT).
    Unversioned,
    /// `<key>.v<N>.sqlite`.
    Versioned(i64),
}

fn generation(file_name: &str) -> Option<Generation> {
    let stem = file_name.strip_suffix(".sqlite")?;
    if stem.is_empty() {
        return None;
    }
    let versioned = stem.rsplit_once(".v").and_then(|(key, version)| {
        (!key.is_empty() && !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit()))
            .then(|| version.parse().ok())
            .flatten()
    });
    Some(versioned.map_or(Generation::Unversioned, Generation::Versioned))
}

/// Whether a file of this generation belongs to an **older** schema than this build's.
/// Newer generations belong to a newer build that may be running; they are never stale.
fn is_stale(generation: Generation) -> bool {
    match generation {
        Generation::Unversioned => true,
        Generation::Versioned(version) => version < SCHEMA_VERSION,
    }
}

/// The outcome of trying to take a stale database file for removal.
enum Probe {
    /// We hold an exclusive lock: nobody else has it open.
    Idle(Connection),
    /// Not a usable database at all, so no process can be using it as an index.
    Corrupt,
    /// Another connection holds it (or it could not be probed): keep it.
    InUse,
}

fn probe(path: &Path) -> Probe {
    let attempt = || -> Result<Connection, crate::IndexError> {
        // Never create: a file that vanished meanwhile is simply not ours to prune.
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // Fail immediately instead of waiting for a live holder to let go.
        conn.busy_timeout(Duration::ZERO)?;
        // In exclusive locking mode the first transaction takes (and keeps) an exclusive
        // lock on the database file itself — in WAL mode too — which conflicts with the
        // shared lock every other open connection holds.
        conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
        conn.execute_batch("BEGIN EXCLUSIVE; COMMIT;")?;
        Ok(conn)
    };
    match attempt() {
        Ok(conn) => Probe::Idle(conn),
        Err(error) if is_corruption(&error) => Probe::Corrupt,
        Err(_) => Probe::InUse,
    }
}

fn remove_with_sidecars(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    for suffix in ["-wal", "-shm", "-journal", crate::OPEN_LOCK_SUFFIX] {
        let _ = std::fs::remove_file(sidecar(path, suffix));
    }
    Ok(())
}

/// Remove index files in `dir` that belong to an older schema generation than this
/// build's (including unversioned pre-HS2-8ZM4PT files) and that no live connection holds
/// open. The current generation, newer generations, files in use, and anything that is
/// not an index database are left untouched. A missing directory is an empty report.
///
/// Safe to call concurrently with servers and CLIs of any build: a file any of them has
/// open is reported in [`PruneReport::in_use`] and kept for a later prune.
pub fn prune_stale_index_files(dir: &Path) -> std::io::Result<PruneReport> {
    let mut report = PruneReport::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(report),
        Err(error) => return Err(error),
    };
    let mut stale: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(generation)
                .is_some_and(is_stale)
        })
        .map(|entry| entry.path())
        .collect();
    stale.sort();
    for path in stale {
        match probe(&path) {
            Probe::Idle(conn) => {
                // Unlink while the exclusive lock is still ours, then let go.
                remove_with_sidecars(&path)?;
                drop(conn);
                report.removed.push(path);
            }
            Probe::Corrupt => {
                remove_with_sidecars(&path)?;
                report.removed.push(path);
            }
            Probe::InUse => report.in_use.push(path),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generations_parse_from_file_names() {
        assert_eq!(generation("abc.sqlite"), Some(Generation::Unversioned));
        assert_eq!(
            generation("abc.v17.sqlite"),
            Some(Generation::Versioned(17))
        );
        assert_eq!(generation("a.b.v3.sqlite"), Some(Generation::Versioned(3)));
        // A `.v` that is not followed by digits is part of an unversioned key.
        assert_eq!(generation("abc.vx.sqlite"), Some(Generation::Unversioned));
        assert_eq!(generation(".v17.sqlite"), Some(Generation::Unversioned));
        assert_eq!(generation("abc.v.sqlite"), Some(Generation::Unversioned));
        assert_eq!(generation("abc.v17.sqlite-wal"), None);
        assert_eq!(generation("abc.v17.sqlite-shm"), None);
        assert_eq!(generation("notes.txt"), None);
        assert_eq!(generation(".sqlite"), None);
    }

    #[test]
    fn only_older_generations_are_stale() {
        assert!(is_stale(Generation::Unversioned));
        assert!(is_stale(Generation::Versioned(SCHEMA_VERSION - 1)));
        assert!(!is_stale(Generation::Versioned(SCHEMA_VERSION)));
        assert!(!is_stale(Generation::Versioned(SCHEMA_VERSION + 1)));
    }
}
