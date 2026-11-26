//! Server-owned GitHub device-flow sessions and their lifetime (HS2-NNWG8H).
//!
//! Only an in-flight flow (still `Pending` and before GitHub's `expires_in` deadline)
//! counts as active work for a safe restart or a repository compaction. A finished flow
//! (authorized, denied, expired, cancelled, or failed) never blocks. Pruning runs on
//! access, never on a timer: every lookup and every quiescence check first marks overdue
//! pending flows `Expired` (waking any waiter and stopping their poller) and drops every
//! session whose retention ended, so an expiry with no further GitHub polling is still
//! pruned on the next check.

use crate::remote_auth::Clock;
use hotsheet_sync::LockExt;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// How long a session stays readable after its device code expires, so a sign-in that
/// finished just before the deadline can still list its repositories.
pub(crate) const FINISHED_RETENTION_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum GitHubAuthStatus {
    Pending,
    Authorized { credential_reference: String },
    Denied,
    Expired,
    Cancelled,
    Error { message: String },
}

pub(crate) struct GitHubAuthSession {
    pub(crate) status: tokio::sync::watch::Sender<GitHubAuthStatus>,
    pub(crate) client_id: String,
    pub(crate) web_base: String,
    pub(crate) credential_reference: String,
    /// Unix seconds at which GitHub's device code expires (`expires_in` after start).
    pub(crate) expires_at: i64,
}

impl GitHubAuthSession {
    pub(crate) fn new(
        status: GitHubAuthStatus,
        client_id: String,
        web_base: String,
        credential_reference: String,
        expires_at: i64,
    ) -> Self {
        let (status, _) = tokio::sync::watch::channel(status);
        Self {
            status,
            client_id,
            web_base,
            credential_reference,
            expires_at,
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        matches!(*self.status.borrow(), GitHubAuthStatus::Pending)
    }

    /// Record a pending flow's outcome and wake its waiters. The first outcome wins, so a
    /// late poll result never overwrites a cancellation or vice versa. Unlike
    /// `watch::Sender::send`, this updates the value even when nobody is waiting: a flow
    /// that finished with no subscriber must not stay `Pending` and block restart.
    pub(crate) fn finish(&self, outcome: GitHubAuthStatus) -> bool {
        self.status.send_if_modified(|status| {
            if matches!(status, GitHubAuthStatus::Pending) {
                *status = outcome;
                true
            } else {
                false
            }
        })
    }

    /// Move a pending flow whose device code has expired to `Expired`. Returns whether
    /// the flow is (now) finished. A flow that already finished keeps its outcome.
    pub(crate) fn expire_if_overdue(&self, now: i64) -> bool {
        if now >= self.expires_at {
            self.finish(GitHubAuthStatus::Expired);
        }
        !self.is_pending()
    }
}

/// The device-flow sessions this server owns, keyed by session id.
pub(crate) struct GitHubAuthSessions {
    sessions: Mutex<HashMap<String, Arc<GitHubAuthSession>>>,
    clock: Clock,
}

impl Default for GitHubAuthSessions {
    fn default() -> Self {
        Self::new(Arc::new(crate::remote_auth::wall_clock))
    }
}

impl GitHubAuthSessions {
    pub(crate) fn new(clock: Clock) -> Self {
        Self {
            sessions: Mutex::default(),
            clock,
        }
    }

    pub(crate) fn now(&self) -> i64 {
        (self.clock)()
    }

    pub(crate) fn clock(&self) -> Clock {
        self.clock.clone()
    }

    pub(crate) fn insert(&self, session_id: String, session: Arc<GitHubAuthSession>) {
        self.sessions.with_lock(|sessions| {
            Self::prune_locked(sessions, self.now());
            sessions.insert(session_id, session);
        });
    }

    pub(crate) fn get(&self, session_id: &str) -> Option<Arc<GitHubAuthSession>> {
        self.sessions.with_lock(|sessions| {
            Self::prune_locked(sessions, self.now());
            sessions.get(session_id).cloned()
        })
    }

    /// Flows that still block a safe restart or compaction: pending and not yet expired.
    pub(crate) fn in_flight(&self) -> usize {
        self.sessions.with_lock(|sessions| {
            Self::prune_locked(sessions, self.now());
            sessions
                .values()
                .filter(|session| session.is_pending())
                .count()
        })
    }

    #[cfg(test)]
    pub(crate) fn retained(&self) -> usize {
        self.sessions.with_lock(|sessions| {
            Self::prune_locked(sessions, self.now());
            sessions.len()
        })
    }

    fn prune_locked(sessions: &mut HashMap<String, Arc<GitHubAuthSession>>, now: i64) {
        sessions.retain(|_, session| {
            session.expire_if_overdue(now);
            now < session.expires_at.saturating_add(FINISHED_RETENTION_SECS)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_auth::ManualClock;

    const START: i64 = 1_800_000_000;
    const EXPIRES_IN: i64 = 900;

    fn session(status: GitHubAuthStatus) -> Arc<GitHubAuthSession> {
        Arc::new(GitHubAuthSession::new(
            status,
            "Iv1synthetic00".into(),
            "https://github.example".into(),
            "github-app-synthetic".into(),
            START + EXPIRES_IN,
        ))
    }

    fn terminal_states() -> Vec<GitHubAuthStatus> {
        vec![
            GitHubAuthStatus::Authorized {
                credential_reference: "github-app-synthetic".into(),
            },
            GitHubAuthStatus::Denied,
            GitHubAuthStatus::Expired,
            GitHubAuthStatus::Cancelled,
            GitHubAuthStatus::Error {
                message: "synthetic failure".into(),
            },
        ]
    }

    #[test]
    fn hs2_nnwg8h_each_terminal_state_stops_blocking() {
        for terminal in terminal_states() {
            let clock = ManualClock::new(START);
            let sessions = GitHubAuthSessions::new(clock.clock());
            let flow = session(GitHubAuthStatus::Pending);
            sessions.insert("flow".into(), flow.clone());
            assert_eq!(sessions.in_flight(), 1, "{terminal:?}");
            flow.finish(terminal.clone());
            assert_eq!(sessions.in_flight(), 0, "{terminal:?}");
            // Still readable for a follow-up (repository listing) until retention ends.
            assert_eq!(*sessions.get("flow").unwrap().status.borrow(), terminal);
            clock.set(START + EXPIRES_IN + FINISHED_RETENTION_SECS);
            assert!(sessions.get("flow").is_none(), "{terminal:?}");
            assert_eq!(sessions.retained(), 0);
        }
    }

    #[test]
    fn hs2_nnwg8h_an_outcome_lands_without_a_waiter_and_the_first_one_wins() {
        let clock = ManualClock::new(START);
        let sessions = GitHubAuthSessions::new(clock.clock());
        let flow = session(GitHubAuthStatus::Pending);
        sessions.insert("flow".into(), flow.clone());
        // No subscriber: `watch::Sender::send` would drop this and leave the flow blocking.
        assert_eq!(flow.status.receiver_count(), 0);
        assert!(flow.finish(GitHubAuthStatus::Denied));
        assert_eq!(sessions.in_flight(), 0);
        // A late cancel or poll result never overwrites the recorded outcome.
        assert!(!flow.finish(GitHubAuthStatus::Cancelled));
        clock.set(START + EXPIRES_IN);
        assert!(flow.expire_if_overdue(clock.now()));
        assert_eq!(*flow.status.borrow(), GitHubAuthStatus::Denied);
    }

    #[test]
    fn hs2_nnwg8h_in_flight_flow_blocks_until_its_deadline() {
        let clock = ManualClock::new(START);
        let sessions = GitHubAuthSessions::new(clock.clock());
        let flow = session(GitHubAuthStatus::Pending);
        sessions.insert("flow".into(), flow.clone());
        clock.set(START + EXPIRES_IN - 1);
        assert_eq!(sessions.in_flight(), 1);
        assert!(flow.is_pending());
        // The deadline passes with no GitHub poll: the next check expires and frees it.
        clock.advance(1);
        assert_eq!(sessions.in_flight(), 0);
        assert_eq!(*flow.status.borrow(), GitHubAuthStatus::Expired);
        assert_eq!(sessions.retained(), 1);
        clock.advance(FINISHED_RETENTION_SECS);
        assert_eq!(sessions.retained(), 0);
    }

    #[test]
    fn hs2_nnwg8h_overdue_expiry_wakes_a_waiter_and_keeps_finished_outcomes() {
        let clock = ManualClock::new(START);
        let sessions = GitHubAuthSessions::new(clock.clock());
        let pending = session(GitHubAuthStatus::Pending);
        let denied = session(GitHubAuthStatus::Denied);
        sessions.insert("pending".into(), pending.clone());
        sessions.insert("denied".into(), denied.clone());
        let mut waiter = pending.status.subscribe();
        clock.set(START + EXPIRES_IN + 5);
        assert!(sessions.get("pending").is_some());
        assert!(waiter.has_changed().unwrap());
        assert_eq!(*waiter.borrow_and_update(), GitHubAuthStatus::Expired);
        // A flow that finished before the deadline keeps its outcome.
        assert_eq!(*denied.status.borrow(), GitHubAuthStatus::Denied);
    }

    #[test]
    fn hs2_nnwg8h_concurrent_flows_count_independently_and_refill() {
        let clock = ManualClock::new(START);
        let sessions = GitHubAuthSessions::new(clock.clock());
        let flows: Vec<_> = (0..4)
            .map(|index| {
                let flow = session(GitHubAuthStatus::Pending);
                sessions.insert(format!("flow-{index}"), flow.clone());
                flow
            })
            .collect();
        assert_eq!(sessions.in_flight(), 4);
        for (finished, (flow, terminal)) in flows.iter().zip(terminal_states()).enumerate() {
            flow.finish(terminal);
            assert_eq!(sessions.in_flight(), 3 - finished);
        }
        // Empty, then refill: a later flow blocks again and finishes again.
        let later = session(GitHubAuthStatus::Pending);
        sessions.insert("later".into(), later.clone());
        assert_eq!(sessions.in_flight(), 1);
        later.finish(GitHubAuthStatus::Cancelled);
        assert_eq!(sessions.in_flight(), 0);
        assert_eq!(sessions.retained(), 5);
    }

    #[test]
    fn hs2_nnwg8h_a_new_flow_after_old_ones_expire_prunes_the_old_ones() {
        let clock = ManualClock::new(START);
        let sessions = GitHubAuthSessions::new(clock.clock());
        sessions.insert("old".into(), session(GitHubAuthStatus::Pending));
        clock.set(START + EXPIRES_IN + FINISHED_RETENTION_SECS);
        let fresh = Arc::new(GitHubAuthSession::new(
            GitHubAuthStatus::Pending,
            "Iv1synthetic00".into(),
            "https://github.example".into(),
            "github-app-fresh".into(),
            clock.now() + EXPIRES_IN,
        ));
        sessions.insert("fresh".into(), fresh);
        assert!(sessions.get("old").is_none());
        assert_eq!(sessions.in_flight(), 1);
        assert_eq!(sessions.retained(), 1);
    }
}
