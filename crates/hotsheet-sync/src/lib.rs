//! Shared synchronization helpers (HS2-ZGQJZP).
//!
//! Hot Sheet's mutexes guard plain in-process state (caches, registries, buffers) whose
//! invariants hold between statements, so a panic in one request or worker thread must not
//! cascade into every later caller. The single policy is therefore: **recover a poisoned
//! lock and keep serving**. Call [`LockExt::lock_or_recover`] (or the `RwLock` variants)
//! instead of `lock().unwrap()`, silently skipping via `if let Ok(..)`, or mapping poison
//! to an HTTP 500.

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Poison-recovering lock acquisition for [`Mutex`].
pub trait LockExt<T: ?Sized> {
    /// Lock the mutex, recovering the guard if a previous holder panicked.
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;

    /// Run `f` with the (poison-recovered) locked value and return its result, releasing
    /// the lock before returning.
    fn with_lock<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(&mut self.lock_or_recover())
    }
}

impl<T: ?Sized> LockExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Poison-recovering lock acquisition for [`RwLock`].
pub trait RwLockExt<T: ?Sized> {
    /// Take a shared read guard, recovering it if a writer panicked.
    fn read_or_recover(&self) -> RwLockReadGuard<'_, T>;
    /// Take an exclusive write guard, recovering it if a previous holder panicked.
    fn write_or_recover(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T: ?Sized> RwLockExt<T> for RwLock<T> {
    fn read_or_recover(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_or_recover(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::{LockExt, RwLockExt};
    use std::sync::{Arc, Mutex, RwLock};

    #[test]
    fn a_poisoned_mutex_is_recovered_with_its_last_state() {
        let lock = Arc::new(Mutex::new(1));
        let poisoner = lock.clone();
        let _ = std::thread::spawn(move || {
            let mut guard = poisoner.lock().unwrap();
            *guard = 2;
            panic!("poison");
        })
        .join();
        assert!(lock.is_poisoned());
        assert_eq!(*lock.lock_or_recover(), 2);
        *lock.lock_or_recover() = 3;
        assert_eq!(*lock.lock_or_recover(), 3);
    }

    #[test]
    fn a_poisoned_rwlock_is_recovered_for_readers_and_writers() {
        let lock = Arc::new(RwLock::new(vec![1]));
        let poisoner = lock.clone();
        let _ = std::thread::spawn(move || {
            let mut guard = poisoner.write().unwrap();
            guard.push(2);
            panic!("poison");
        })
        .join();
        assert!(lock.is_poisoned());
        assert_eq!(*lock.read_or_recover(), vec![1, 2]);
        lock.write_or_recover().push(3);
        assert_eq!(lock.read_or_recover().len(), 3);
    }

    #[test]
    fn an_unpoisoned_lock_behaves_like_lock() {
        let lock = Mutex::new("ok");
        assert_eq!(*lock.lock_or_recover(), "ok");
    }

    #[test]
    fn with_lock_runs_against_a_recovered_value_and_releases_it() {
        let lock = Arc::new(Mutex::new(vec![1]));
        let poisoner = lock.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert_eq!(
            lock.with_lock(|values| {
                values.push(2);
                values.len()
            }),
            2
        );
        // The guard was released: a second acquisition does not deadlock.
        assert_eq!(lock.with_lock(|values| values.clone()), vec![1, 2]);
    }
}
