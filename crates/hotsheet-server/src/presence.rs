//! Which checkouts a client currently has open (HS2-ARJ9J1).
//!
//! Closing a project happens in the client, so the server infers "open" from live change-stream
//! subscriptions: every `/ws/sync` socket and `/ws/poll` request names its checkout and the
//! browser tab (client) it serves. A lease stays live while a socket is open, while a poll is in
//! flight, and for [`POLL_RECONNECT_GAP`] after either ends, covering the pause between
//! long-polls and a reconnect. A client closing a project drops its lease at once. A
//! subscription that names no checkout (an older client) pins every store, so version skew can
//! never unhost a store a client still shows.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a lease outlives its last socket or poll: a long-poll client re-polls well within it.
pub const POLL_RECONNECT_GAP: Duration = Duration::from_secs(60);
/// How long a store stays hosted after its last lease lapses without an explicit close.
pub const UNHOST_GRACE: Duration = Duration::from_secs(600);
/// The checkout key for subscriptions that name none; such a live lease pins every store.
const UNTAGGED: &str = "*";

#[derive(Debug, Default)]
struct Lease {
    sockets: u32,
    polls: u32,
    last_seen: Option<Instant>,
}

impl Lease {
    fn live(&self, now: Instant) -> bool {
        self.sockets > 0
            || self.polls > 0
            || self
                .last_seen
                .is_some_and(|seen| now.saturating_duration_since(seen) < POLL_RECONNECT_GAP)
    }
}

/// A change-stream subscription's transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Channel {
    Socket,
    Poll,
}

/// The live leases, shared by every clone.
#[derive(Clone, Default)]
pub struct Presence {
    leases: Arc<Mutex<HashMap<(String, String), Lease>>>,
}

/// What is open right now.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LiveCheckouts {
    pub checkouts: HashSet<String>,
    /// A live subscription named no checkout, so nothing may be unhosted.
    pub untagged: bool,
}

impl Presence {
    fn key(checkout: Option<&str>, client: Option<&str>) -> (String, String) {
        (
            checkout
                .filter(|value| !value.is_empty())
                .unwrap_or(UNTAGGED)
                .to_string(),
            client.unwrap_or_default().to_string(),
        )
    }

    /// Record a subscription starting; the lease holds until the guard drops.
    pub fn begin(
        &self,
        checkout: Option<&str>,
        client: Option<&str>,
        channel: Channel,
    ) -> PresenceGuard {
        self.begin_at(checkout, client, channel, Instant::now())
    }

    fn begin_at(
        &self,
        checkout: Option<&str>,
        client: Option<&str>,
        channel: Channel,
        now: Instant,
    ) -> PresenceGuard {
        let key = Self::key(checkout, client);
        if let Ok(mut leases) = self.leases.lock() {
            let lease = leases.entry(key.clone()).or_default();
            match channel {
                Channel::Socket => lease.sockets += 1,
                Channel::Poll => lease.polls += 1,
            }
            lease.last_seen = Some(now);
        }
        PresenceGuard {
            presence: self.clone(),
            key,
            channel,
        }
    }

    fn end_at(&self, key: &(String, String), channel: Channel, now: Instant) {
        if let Ok(mut leases) = self.leases.lock()
            && let Some(lease) = leases.get_mut(key)
        {
            match channel {
                Channel::Socket => lease.sockets = lease.sockets.saturating_sub(1),
                Channel::Poll => lease.polls = lease.polls.saturating_sub(1),
            }
            lease.last_seen = Some(now);
        }
    }

    /// A client closed a project: its lease on that checkout ends now, not after the gap.
    pub fn close(&self, checkout: &str, client: Option<&str>) {
        if let Ok(mut leases) = self.leases.lock() {
            leases.remove(&Self::key(Some(checkout), client));
        }
    }

    /// The checkouts with a live lease, dropping lapsed leases as it goes.
    pub fn live(&self) -> LiveCheckouts {
        self.live_at(Instant::now())
    }

    fn live_at(&self, now: Instant) -> LiveCheckouts {
        let mut live = LiveCheckouts::default();
        if let Ok(mut leases) = self.leases.lock() {
            leases.retain(|_, lease| lease.live(now));
            for (checkout, _) in leases.keys() {
                if checkout == UNTAGGED {
                    live.untagged = true;
                } else {
                    live.checkouts.insert(checkout.clone());
                }
            }
        }
        live
    }
}

/// Holds a subscription's lease until dropped (a finished or aborted poll, a closed socket).
pub struct PresenceGuard {
    presence: Presence,
    key: (String, String),
    channel: Channel,
}

impl Drop for PresenceGuard {
    fn drop(&mut self) {
        self.presence
            .end_at(&self.key, self.channel, Instant::now());
    }
}

/// The hosted stores that may be unhosted: each eligible store with the checkouts that reference
/// it, less those referenced by a live checkout or still busy (live work in them).
pub fn unhost_candidates(
    eligible: &[(String, Vec<String>)],
    live: &LiveCheckouts,
    busy: &HashSet<String>,
) -> Vec<String> {
    if live.untagged {
        return Vec::new();
    }
    eligible
        .iter()
        .filter(|(store, checkouts)| {
            !busy.contains(store)
                && !checkouts
                    .iter()
                    .any(|checkout| live.checkouts.contains(checkout))
        })
        .map(|(store, _)| store.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn a_poll_lease_outlives_its_request_by_the_reconnect_gap() {
        let presence = Presence::default();
        let start = Instant::now();
        let guard = presence.begin_at(Some("app"), Some("tab-1"), Channel::Poll, start);
        assert!(presence.live_at(at(start, 3_600)).checkouts.contains("app"));
        let key = guard.key.clone();
        std::mem::forget(guard);
        presence.end_at(&key, Channel::Poll, at(start, 25));
        assert!(presence.live_at(at(start, 80)).checkouts.contains("app"));
        assert!(presence.live_at(at(start, 86)).checkouts.is_empty());
    }

    #[test]
    fn a_socket_lease_lasts_while_the_socket_is_open() {
        let presence = Presence::default();
        let start = Instant::now();
        let guard = presence.begin_at(Some("app"), Some("tab-1"), Channel::Socket, start);
        assert!(presence.live_at(at(start, 7_200)).checkouts.contains("app"));
        drop(guard);
        assert!(
            presence.live().checkouts.contains("app"),
            "the gap applies after close"
        );
    }

    #[test]
    fn closing_drops_only_that_clients_lease_immediately() {
        let presence = Presence::default();
        let _first = presence.begin(Some("app"), Some("tab-1"), Channel::Socket);
        let _second = presence.begin(Some("app"), Some("tab-2"), Channel::Poll);
        presence.close("app", Some("tab-1"));
        assert!(
            presence.live().checkouts.contains("app"),
            "another tab still has the project open"
        );
        presence.close("app", Some("tab-2"));
        assert!(presence.live().checkouts.is_empty());
        // Reopening after a close starts a fresh lease.
        let _again = presence.begin(Some("app"), Some("tab-1"), Channel::Poll);
        assert!(presence.live().checkouts.contains("app"));
    }

    #[test]
    fn an_untagged_subscription_pins_everything() {
        let presence = Presence::default();
        let guard = presence.begin(None, None, Channel::Poll);
        let live = presence.live();
        assert!(live.untagged);
        let eligible = vec![("store-a".to_string(), vec!["app".to_string()])];
        assert!(unhost_candidates(&eligible, &live, &HashSet::new()).is_empty());
        drop(guard);
    }

    #[test]
    fn candidates_skip_live_and_busy_stores() {
        let live = LiveCheckouts {
            checkouts: ["open".to_string()].into(),
            untagged: false,
        };
        let eligible = vec![
            ("shared".to_string(), vec!["open".into(), "closed".into()]),
            ("closed-only".to_string(), vec!["closed".into()]),
            ("busy".to_string(), vec!["closed".into()]),
            ("orphan".to_string(), vec![]),
        ];
        let busy: HashSet<String> = ["busy".to_string()].into();
        assert_eq!(
            unhost_candidates(&eligible, &live, &busy),
            ["closed-only", "orphan"]
        );
    }
}
