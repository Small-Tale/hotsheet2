//! HS2-A8TTD7: re-entrant store transactions, ordered multi-store locking, and ticket
//! snapshots. Every store is a temporary directory.

use std::sync::mpsc;
use std::time::Duration;

use hotsheet_model::{Ticket, Ulid, derive_slug};

use super::*;
#[cfg(unix)]
use crate::FileLock;
use crate::StoreMetadata;
#[cfg(unix)]
use crate::store::NOTE_MUTATION_LOCK_FILE;

fn temp_store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, store)
}

fn sample(id: &str, title: &str) -> Ticket {
    let id = Ulid::from_string(id).unwrap();
    Ticket::new(
        id,
        derive_slug(&id, "HS"),
        title,
        "bug",
        "2026-08-19T00:00:00Z",
        "2026-08-19T00:00:00Z",
    )
}

/// Whether another lock holder (here a second open file description) would wait.
#[cfg(unix)]
fn store_lock_busy(store: &FsStore) -> bool {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(store.root().join(NOTE_MUTATION_LOCK_FILE))
        .unwrap();
    FileLock::try_acquire(file).unwrap().is_none()
}

/// Run `operation` on its own thread and fail instead of hanging if it deadlocks.
fn finishes<T: Send + 'static>(operation: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || sender.send(operation()).unwrap());
    receiver
        .recv_timeout(Duration::from_secs(30))
        .expect("nested store transaction deadlocked")
}

#[test]
fn hs2_a8ttd7_a_nested_transaction_on_the_same_store_joins_the_outer_one() {
    let (_dir, store) = temp_store();
    let written = finishes(move || {
        store
            .with_ticket_transaction(|| {
                // A second spelling of the same root is the same lock.
                let alias = FsStore::open(store.root().join(".")).unwrap();
                alias.with_ticket_transaction(|| {
                    store.write_ticket_committing(&sample("01ARZ3NDEKTSV4RRFFQ69G5FAV", "nested"))
                })?;
                Ok::<_, StoreError>(store.root().to_path_buf())
            })
            .unwrap()
    });
    let key = lock_key(&written);
    assert!(!held_by_this_thread(&key));
}

#[cfg(unix)]
#[test]
fn hs2_a8ttd7_stores_stay_locked_for_the_whole_multi_store_transaction() {
    let (_one_dir, one) = temp_store();
    let (_two_dir, two) = temp_store();
    // Listing a store twice locks it once; the nested single-store ops re-enter.
    with_stores_transaction(&[&two, &one, &two], || {
        assert!(store_lock_busy(&one));
        assert!(store_lock_busy(&two));
        one.with_ticket_transaction(|| {
            one.write_ticket_committing(&sample("01ARZ3NDEKTSV4RRFFQ69G5FAV", "one"))
        })?;
        two.with_ticket_transaction(|| {
            two.write_ticket_committing(&sample("01ARZ3NDEKTSV4RRFFQ69G5FAW", "two"))
        })?;
        // Still held after the nested levels finished.
        assert!(store_lock_busy(&one));
        assert!(store_lock_busy(&two));
        Ok::<_, StoreError>(())
    })
    .unwrap();
    assert!(!store_lock_busy(&one));
    assert!(!store_lock_busy(&two));
    assert!(!held_by_this_thread(&lock_key(one.root())));
}

#[cfg(unix)]
#[test]
fn hs2_a8ttd7_a_panic_releases_the_held_marker_and_the_lock() {
    let (_dir, store) = temp_store();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_stores_transaction(&[&store], || -> Result<(), StoreError> {
            panic!("operation panicked")
        })
    }));
    assert!(panicked.is_err());
    assert!(!held_by_this_thread(&lock_key(store.root())));
    assert!(!store_lock_busy(&store));
}

#[test]
fn hs2_a8ttd7_an_empty_store_list_runs_the_operation_unlocked() {
    assert_eq!(
        with_stores_transaction(&[], || Ok::<_, StoreError>(7)).unwrap(),
        7
    );
}

#[test]
fn hs2_a8ttd7_a_snapshot_restores_the_exact_file_or_its_absence() {
    let (_dir, store) = temp_store();
    let existing = sample("01ARZ3NDEKTSV4RRFFQ69G5FAV", "before");
    store.write_ticket(&existing).unwrap();
    let path = store.ticket_path(&existing.id);
    let before = fs::read(&path).unwrap();
    let snapshot = TicketFileSnapshot::take(&store, &existing.id).unwrap();
    store
        .write_ticket(&sample("01ARZ3NDEKTSV4RRFFQ69G5FAV", "after"))
        .unwrap();
    assert_ne!(fs::read(&path).unwrap(), before);
    snapshot.restore().unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    // Restoring twice is harmless.
    snapshot.restore().unwrap();
    assert_eq!(store.read_ticket(&existing.id).unwrap().title, "before");

    let missing = sample("01ARZ3NDEKTSV4RRFFQ69G5FAW", "new");
    let absent = TicketFileSnapshot::take(&store, &missing.id).unwrap();
    store.write_ticket(&missing).unwrap();
    absent.restore().unwrap();
    assert!(!store.ticket_path(&missing.id).exists());
    absent.restore().unwrap();
}
