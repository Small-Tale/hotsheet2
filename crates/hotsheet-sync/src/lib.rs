//! Shared synchronization helpers (HS2-ZGQJZP).
//!
//! Hot Sheet's mutexes guard plain in-process state (caches, registries, buffers) whose
//! invariants hold between statements, so a panic in one request or worker thread must not
//! cascade into every later caller. The single policy is therefore: **recover a poisoned
//! lock and keep serving**. Call [`LockExt::lock_or_recover`] (or the `RwLock` variants)
//! instead of `lock().unwrap()`, silently skipping via `if let Ok(..)`, or mapping poison
//! to an HTTP 500. A [`Condvar`] wait re-acquires its mutex and reports poison the same way,
//! so it goes through [`CondvarExt`] (HS2-GSW2AA). `tests/lock_poison_policy.rs` keeps
//! production code free of `.lock().unwrap()` and `wait*(..).unwrap()`.
//!
//! [`env_path`] holds the other cross-crate leaf rule: a blank path-valued environment
//! override counts as unset (HS2-15ZN8E). [`files`] holds the one atomic file
//! replacement that works on Windows while readers hold the target open (HS2-J5DCJH).
//! [`secret::SecretString`] is the one credential type whose `Debug` prints `<redacted>`
//! (HS2-A8T481).

pub mod env_path;
pub mod files;
pub mod paths;
pub mod secret;

use std::sync::{
    Condvar, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
    WaitTimeoutResult,
};
use std::time::Duration;

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

/// Poison-recovering waits for [`Condvar`] (HS2-GSW2AA).
///
/// A condition-variable wait re-acquires its mutex before returning, and reports a poisoned
/// mutex as an error exactly like `lock()`. Each method recovers the guard, so a panic in one
/// thread holding the mutex never cascades into every waiter.
pub trait CondvarExt {
    /// [`Condvar::wait`], recovering a poisoned guard.
    fn wait_or_recover<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T>;
    /// [`Condvar::wait_timeout`], recovering a poisoned guard.
    fn wait_timeout_or_recover<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        timeout: Duration,
    ) -> (MutexGuard<'a, T>, WaitTimeoutResult);
    /// [`Condvar::wait_timeout_while`], recovering a poisoned guard.
    fn wait_timeout_while_or_recover<'a, T, F>(
        &self,
        guard: MutexGuard<'a, T>,
        timeout: Duration,
        condition: F,
    ) -> (MutexGuard<'a, T>, WaitTimeoutResult)
    where
        F: FnMut(&mut T) -> bool;
}

impl CondvarExt for Condvar {
    fn wait_or_recover<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        self.wait(guard).unwrap_or_else(PoisonError::into_inner)
    }

    fn wait_timeout_or_recover<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        timeout: Duration,
    ) -> (MutexGuard<'a, T>, WaitTimeoutResult) {
        self.wait_timeout(guard, timeout)
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn wait_timeout_while_or_recover<'a, T, F>(
        &self,
        guard: MutexGuard<'a, T>,
        timeout: Duration,
        condition: F,
    ) -> (MutexGuard<'a, T>, WaitTimeoutResult)
    where
        F: FnMut(&mut T) -> bool,
    {
        self.wait_timeout_while(guard, timeout, condition)
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::{CondvarExt, LockExt, RwLockExt};
    use std::sync::{Arc, Condvar, Mutex, RwLock};
    use std::time::Duration;

    /// A mutex poisoned by a panicking holder, plus its condition variable.
    fn poisoned_pair(value: u32) -> Arc<(Mutex<u32>, Condvar)> {
        let pair = Arc::new((Mutex::new(value), Condvar::new()));
        let poisoner = pair.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.0.lock();
            panic!("poison");
        })
        .join();
        assert!(pair.0.is_poisoned());
        pair
    }

    #[test]
    fn hs2_gsw2aa_a_wait_on_a_poisoned_mutex_recovers_and_sees_the_notified_state() {
        let pair = poisoned_pair(0);
        let notifier = pair.clone();
        let guard = pair.0.lock_or_recover();
        // The notifier can only take the lock once this thread is parked in the wait, so the
        // wait observes its write and wakes on its notify, not on a timer.
        let handle = std::thread::spawn(move || {
            *notifier.0.lock_or_recover() = 1;
            notifier.1.notify_all();
        });
        let mut guard = guard;
        while *guard == 0 {
            guard = pair.1.wait_or_recover(guard);
        }
        assert_eq!(*guard, 1);
        drop(guard);
        handle.join().unwrap();
    }

    #[test]
    fn hs2_gsw2aa_timed_waits_on_a_poisoned_mutex_recover_the_guard() {
        let pair = poisoned_pair(7);
        let (guard, result) = pair
            .1
            .wait_timeout_or_recover(pair.0.lock_or_recover(), Duration::from_millis(1));
        assert!(result.timed_out());
        assert_eq!(*guard, 7);
        let (guard, result) =
            pair.1
                .wait_timeout_while_or_recover(guard, Duration::from_millis(1), |value| {
                    *value != 7
                });
        // The condition already holds, so the wait returns at once without timing out.
        assert!(!result.timed_out());
        assert_eq!(*guard, 7);
        drop(guard);
        assert_eq!(*pair.0.lock_or_recover(), 7);
    }

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
