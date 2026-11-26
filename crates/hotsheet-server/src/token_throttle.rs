//! A bounded failure budget per peer for the unauthenticated renewal-token route
//! (HS2-JFEMAR, `docs/36-resource-claims.md` §36.11).
//!
//! `POST /resource-claim-token/{action}` sits outside server authentication by design. A token
//! carries a 256-bit secret, so guessing is infeasible, but every attempt took the namespace lock
//! and did file I/O, and an unbounded flood of wrong tokens queued behind itself and slowed the
//! whole server. Measured on loopback in a debug build: a serial wrong token cost 3.4 ms; eight
//! parallel flooders held the route to about 80 attempts per second and slowed a valid renewal
//! from 9 ms to 110 ms at the median; one flooder slowed it to 15 ms (p99 43 ms).
//!
//! The budget bounds that per peer. A peer may fail [`DEFAULT_BUDGET`] times in a window; its
//! further attempts in that window are answered `429 rate_limited` with `Retry-After` and are
//! never evaluated, so the work a peer can cause is bounded. A valid token never counts, and a
//! peer that has not been failing is never delayed, so a guest keeping its lease is unaffected by
//! another peer's flood. A peer that exhausted its own budget waits out the window, which is
//! short next to a lease.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

use hotsheet_sync::LockExt;
use std::time::{Duration, Instant};

/// Failures one peer may cause per window before it is throttled.
pub const DEFAULT_BUDGET: u32 = 30;
/// The window a budget covers.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(10);
/// Peers remembered at once; the least recently started window is dropped first.
const MAX_PEERS: usize = 1024;

#[derive(Debug)]
pub struct FailureBudget {
    budget: u32,
    window: Duration,
    peers: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl Default for FailureBudget {
    fn default() -> Self {
        Self::new(DEFAULT_BUDGET, DEFAULT_WINDOW)
    }
}

impl FailureBudget {
    pub fn new(budget: u32, window: Duration) -> Self {
        Self {
            budget,
            window,
            peers: Mutex::new(HashMap::new()),
        }
    }

    /// `Err(retry_after)` when `peer` has used its budget in the current window.
    pub fn check(&self, peer: IpAddr, now: Instant) -> Result<(), Duration> {
        let peers = self.peers.lock_or_recover();
        match peers.get(&peer) {
            Some((failures, started))
                if now < *started + self.window && *failures >= self.budget =>
            {
                Err((*started + self.window).saturating_duration_since(now))
            }
            _ => Ok(()),
        }
    }

    /// Record one failed attempt by `peer`.
    pub fn failed(&self, peer: IpAddr, now: Instant) {
        let mut peers = self.peers.lock_or_recover();
        if peers.len() >= MAX_PEERS && !peers.contains_key(&peer) {
            peers.retain(|_, (_, started)| now < *started + self.window);
            if peers.len() >= MAX_PEERS
                && let Some(oldest) = peers
                    .iter()
                    .min_by_key(|(_, (_, started))| *started)
                    .map(|(ip, _)| *ip)
            {
                peers.remove(&oldest);
            }
        }
        let entry = peers.entry(peer).or_insert((0, now));
        if now >= entry.1 + self.window {
            *entry = (0, now);
        }
        entry.0 = entry.0.saturating_add(1);
    }

    /// How many peers are remembered (for tests).
    #[cfg(test)]
    pub fn tracked(&self) -> usize {
        self.peers.lock_or_recover().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn hs2_jfemar_a_peer_is_throttled_only_after_its_own_budget_and_only_until_the_window_ends() {
        let budget = FailureBudget::new(3, Duration::from_secs(10));
        let t0 = Instant::now();
        for _ in 0..3 {
            assert!(budget.check(ip(1), t0).is_ok(), "within budget");
            budget.failed(ip(1), t0);
        }
        let retry = budget
            .check(ip(1), t0 + Duration::from_secs(4))
            .unwrap_err();
        assert_eq!(
            retry,
            Duration::from_secs(6),
            "waits out the rest of the window"
        );
        assert!(
            budget.check(ip(2), t0 + Duration::from_secs(4)).is_ok(),
            "another peer is never delayed"
        );
        assert!(
            budget.check(ip(1), t0 + Duration::from_secs(10)).is_ok(),
            "the window ended"
        );
        budget.failed(ip(1), t0 + Duration::from_secs(11));
        assert!(
            budget.check(ip(1), t0 + Duration::from_secs(11)).is_ok(),
            "a new window starts from zero"
        );
    }

    #[test]
    fn hs2_jfemar_failures_before_the_budget_never_throttle_and_a_budget_of_zero_always_does() {
        let t0 = Instant::now();
        let some = FailureBudget::new(5, Duration::from_secs(10));
        for _ in 0..4 {
            some.failed(ip(1), t0);
        }
        assert!(some.check(ip(1), t0).is_ok());
        let zero = FailureBudget::new(0, Duration::from_secs(10));
        assert!(
            zero.check(ip(1), t0).is_ok(),
            "an unseen peer is checked, not refused"
        );
        zero.failed(ip(1), t0);
        assert!(zero.check(ip(1), t0).is_err());
    }

    #[test]
    fn hs2_jfemar_the_peer_table_is_bounded() {
        let budget = FailureBudget::new(1, Duration::from_secs(60));
        let t0 = Instant::now();
        for n in 0..(MAX_PEERS + 200) {
            let n = u32::try_from(n).unwrap();
            budget.failed(
                IpAddr::from([
                    10,
                    1,
                    u8::try_from(n / 256).unwrap(),
                    u8::try_from(n % 256).unwrap(),
                ]),
                t0 + Duration::from_millis(u64::from(n)),
            );
        }
        assert!(budget.tracked() <= MAX_PEERS, "{}", budget.tracked());
        // The most recent peers are still remembered; the oldest were dropped.
        let newest = u32::try_from(MAX_PEERS + 199).unwrap();
        let newest = IpAddr::from([
            10,
            1,
            u8::try_from(newest / 256).unwrap(),
            u8::try_from(newest % 256).unwrap(),
        ]);
        assert!(budget.check(newest, t0 + Duration::from_secs(1)).is_err());
    }

    #[test]
    fn hs2_gsw2aa_a_poisoned_budget_keeps_counting_instead_of_panicking() {
        let budget = std::sync::Arc::new(FailureBudget::new(1, Duration::from_secs(60)));
        let t0 = Instant::now();
        budget.failed(ip(1), t0);
        let poisoner = budget.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.peers.lock();
            panic!("poison the peer table");
        })
        .join();
        assert!(budget.peers.is_poisoned());
        // The failure recorded before the panic still throttles; a new peer is still counted.
        assert!(budget.check(ip(1), t0 + Duration::from_secs(1)).is_err());
        assert!(budget.check(ip(2), t0).is_ok());
        budget.failed(ip(2), t0);
        assert!(budget.check(ip(2), t0 + Duration::from_secs(1)).is_err());
    }
}
