//! Multi-store ticket transactions and ticket-file snapshots (HS2-A8TTD7).
//!
//! A bulk mutation that must land on every ticket or on none holds each involved
//! store's ticket transaction for its whole duration, taken in one stable order so two
//! bulk writers cannot deadlock, and snapshots each ticket file before writing it. A
//! failure part-way restores the snapshots while the locks are still held, so no other
//! writer can observe or build on the partial state.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use hotsheet_model::Ulid;

use crate::store::{FsStore, StoreError, write_file_atomically};

thread_local! {
    /// Store lock keys this thread holds through [`FsStore::with_ticket_transaction`].
    static HELD: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// The identity a store's mutation lock is tracked by: its canonical root, so two
/// spellings of one store directory are one lock.
pub(crate) fn lock_key(root: &Path) -> PathBuf {
    hotsheet_sync::paths::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// Whether this thread already holds the store lock named by `key`.
pub(crate) fn held_by_this_thread(key: &Path) -> bool {
    HELD.with(|held| held.borrow().iter().any(|entry| entry == key))
}

/// Marks a store lock as held by this thread until dropped (including on unwind).
pub(crate) struct Held(PathBuf);

impl Held {
    pub(crate) fn enter(key: PathBuf) -> Self {
        HELD.with(|held| held.borrow_mut().push(key.clone()));
        Self(key)
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        HELD.with(|held| {
            let mut held = held.borrow_mut();
            if let Some(index) = held.iter().rposition(|entry| *entry == self.0) {
                held.remove(index);
            }
        });
    }
}

/// Hold the ticket transaction of every store in `stores` while `operation` runs. Locks
/// are taken in canonical-root order (the order
/// [`FsStore::with_store_pair_transaction`] uses) and a store listed twice is locked
/// once. Single-ticket ops inside `operation` re-enter the held locks.
pub fn with_stores_transaction<T, E: From<StoreError>>(
    stores: &[&FsStore],
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let mut ordered = stores
        .iter()
        .map(|store| (lock_key(store.root()), *store))
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.0.cmp(&right.0));
    ordered.dedup_by(|left, right| left.0 == right.0);
    let ordered = ordered
        .into_iter()
        .map(|(_, store)| store)
        .collect::<Vec<_>>();
    nest(&ordered, operation)
}

fn nest<T, E: From<StoreError>, F: FnOnce() -> Result<T, E>>(
    stores: &[&FsStore],
    operation: F,
) -> Result<T, E> {
    match stores.split_first() {
        None => operation(),
        Some((first, rest)) => first.with_ticket_transaction(|| nest(rest, operation)),
    }
}

/// A ticket file's exact bytes as read before a bulk mutation wrote it.
#[derive(Debug, Clone)]
pub struct TicketFileSnapshot {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}

impl TicketFileSnapshot {
    /// Read the ticket's current file. Take it inside the store's ticket transaction.
    pub fn take(store: &FsStore, id: &Ulid) -> Result<Self, StoreError> {
        let path = store.ticket_path(id);
        let bytes = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(StoreError::IoAt {
                    operation: "snapshotting ticket",
                    path,
                    source,
                });
            }
        };
        Ok(Self { path, bytes })
    }

    /// Put the file back exactly as snapshotted (removing it if it did not exist). Call
    /// it while the transaction that took the snapshot is still held.
    pub fn restore(&self) -> Result<(), StoreError> {
        let result = match &self.bytes {
            Some(bytes) => write_file_atomically(&self.path, bytes),
            None => match fs::remove_file(&self.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        };
        result.map_err(|source| StoreError::IoAt {
            operation: "restoring ticket snapshot",
            path: self.path.clone(),
            source,
        })
    }
}

#[cfg(test)]
#[path = "store_transactions_tests.rs"]
mod tests;
