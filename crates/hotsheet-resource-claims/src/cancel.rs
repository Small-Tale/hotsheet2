//! HS2-2W2PTX: ending one wait from another thread.
//!
//! A [`WaitCancel`] is handed to [`crate::ClaimBackend::wait_free_cancellable`]. While the
//! wait parks it registers a waker here; [`WaitCancel::cancel`] sets the flag and fires
//! every waker, so the wait returns [`crate::ClaimError::Interrupted`] at once and drops
//! its registration. A wait checks the flag after registering its waker, so a cancel that
//! lands before the waker exists is never lost. Unlike [`crate::InterruptGuard`] it is not
//! process-wide: an owner that stops (an orchestration runtime's stop token) ends only its
//! own waits.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Inner {
    cancelled: AtomicBool,
    next: AtomicU64,
    wakers: Mutex<Vec<(u64, Waker)>>,
}

/// A cancel signal for waits. Clones share one signal.
#[derive(Clone, Default)]
pub struct WaitCancel(Arc<Inner>);

impl std::fmt::Debug for WaitCancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaitCancel")
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

/// A registered waker; dropping it deregisters the waker.
pub struct WakerRegistration {
    cancel: WaitCancel,
    id: u64,
}

impl Drop for WakerRegistration {
    fn drop(&mut self) {
        self.cancel
            .wakers()
            .retain(|(registered, _)| *registered != self.id);
    }
}

impl WaitCancel {
    pub fn new() -> Self {
        Self::default()
    }

    fn wakers(&self) -> std::sync::MutexGuard<'_, Vec<(u64, Waker)>> {
        self.0.wakers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether [`Self::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// End every wait using this signal, now and later. Idempotent.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        // Fire outside the lock: a waker may take its own locks.
        let wakers: Vec<Waker> = self
            .wakers()
            .iter()
            .map(|(_, waker)| waker.clone())
            .collect();
        for waker in wakers {
            waker();
        }
    }

    /// Register `waker` to run on [`Self::cancel`] until the registration drops. The
    /// caller re-checks [`Self::is_cancelled`] after this returns.
    pub fn register(&self, waker: impl Fn() + Send + Sync + 'static) -> WakerRegistration {
        let id = self.0.next.fetch_add(1, Ordering::SeqCst);
        self.wakers().push((id, Arc::new(waker)));
        WakerRegistration {
            cancel: self.clone(),
            id,
        }
    }

    /// Wakers registered now (tests).
    pub fn registered(&self) -> usize {
        self.wakers().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn hs2_2w2ptx_cancel_fires_live_wakers_and_stays_set() {
        let cancel = WaitCancel::new();
        let fired = Arc::new(AtomicUsize::new(0));
        let counted = fired.clone();
        let kept = cancel.register(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        let counted = fired.clone();
        let dropped = cancel.register(move || {
            counted.fetch_add(100, Ordering::SeqCst);
        });
        assert_eq!(cancel.registered(), 2);
        drop(dropped);
        assert_eq!(cancel.registered(), 1);
        assert!(!cancel.is_cancelled());
        cancel.clone().cancel();
        assert!(cancel.is_cancelled());
        assert_eq!(fired.load(Ordering::SeqCst), 1);
        cancel.cancel();
        assert_eq!(fired.load(Ordering::SeqCst), 2);
        drop(kept);
        assert_eq!(cancel.registered(), 0);
        // Registering after the cancel leaves the flag set for the caller's re-check.
        let _late = cancel.register(|| {});
        assert!(cancel.is_cancelled());
    }
}
