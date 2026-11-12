//! Injectable time and identity sources. Core Job logic never reads the wall clock or a
//! random source directly, so every lease, retention and ordering rule is testable.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// The machine's wall clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            })
    }
}

/// A clock a test moves by hand.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn at(now_ms: u64) -> Self {
        Self(AtomicU64::new(now_ms))
    }

    pub fn set(&self, now_ms: u64) {
        self.0.store(now_ms, Ordering::SeqCst);
    }

    pub fn advance(&self, by_ms: u64) {
        self.0.fetch_add(by_ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Mints Job ids. An id is a ULID string; the store rejects anything else.
pub trait IdSource: Send + Sync {
    fn next_id(&self, now_ms: u64) -> String;
}

/// Random ULIDs stamped with the injected clock's time.
#[derive(Clone, Copy, Debug, Default)]
pub struct UlidSource;

impl IdSource for UlidSource {
    fn next_id(&self, now_ms: u64) -> String {
        let random = ulid::Ulid::new().random();
        ulid::Ulid::from_parts(now_ms, random).to_string()
    }
}

/// Deterministic ULIDs for tests: the clock's time plus a counter.
#[derive(Debug, Default)]
pub struct SequenceIds(AtomicU64);

impl IdSource for SequenceIds {
    fn next_id(&self, now_ms: u64) -> String {
        let counter = self.0.fetch_add(1, Ordering::SeqCst);
        ulid::Ulid::from_parts(now_ms, u128::from(counter)).to_string()
    }
}
