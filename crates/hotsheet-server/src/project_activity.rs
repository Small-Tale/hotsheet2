//! Per-store AI activity from ticket claims (HS2-2MM07G, `docs/32-notifier-plugins.md`
//! §32.8.1). A store is **active** while any ticket holds a live claim. It has **stopped**
//! when its last claim was released and no claim appeared during the grace period, and it
//! has **stalled** when a claim's lease expired without renewal: the session that held it
//! stopped renewing, so it is likely stuck or gone. Pure state; the router supplies the
//! clock and schedules the checks this returns.

use std::collections::{BTreeMap, HashMap};

use time::{Duration, OffsetDateTime};

/// How long a store must stay without claims before it counts as stopped, so a worker
/// moving to its next ticket does not trigger a notification.
pub const IDLE_GRACE: Duration = Duration::seconds(120);

/// One ticket's claim state as the router observed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimObservation {
    pub ticket: String,
    pub slug: String,
    /// `(worker, lease expiry)` while claimed.
    pub claim: Option<(String, OffsetDateTime)>,
    /// The ticket's status is completed or verified.
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Claim {
    slug: String,
    worker: String,
    expires: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Every claim was released.
    Finished,
    /// A claim's lease expired without renewal and no claim remains.
    Stalled,
}

/// What a stopped store reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopReport {
    pub reason: StopReason,
    /// Tickets completed while the store was active, in completion order.
    pub completed: Vec<String>,
    /// `(slug, worker)` of claims whose lease expired, in expiry order.
    pub stalled: Vec<(String, String)>,
    /// When the active period began.
    pub active_since: OffsetDateTime,
}

#[derive(Debug, Default)]
struct StoreActivity {
    claims: BTreeMap<String, Claim>,
    /// Set while there is an active period to report on.
    active_since: Option<OffsetDateTime>,
    /// When the store last lost its final claim.
    idle_since: Option<OffsetDateTime>,
    completed: Vec<String>,
    stalled: Vec<(String, String)>,
}

#[derive(Debug)]
pub struct ProjectActivity {
    stores: HashMap<String, StoreActivity>,
    grace: Duration,
}

impl Default for ProjectActivity {
    fn default() -> Self {
        ProjectActivity::with_grace(IDLE_GRACE)
    }
}

impl ProjectActivity {
    /// A tracker with a different idle grace (tests use a short one).
    pub fn with_grace(grace: Duration) -> ProjectActivity {
        ProjectActivity {
            stores: HashMap::new(),
            grace,
        }
    }

    /// Record a ticket's claim state. Returns when the router should next call
    /// [`Self::check`] for this store (a lease expiry or the end of the idle grace).
    /// Callers run `check` first, so a period whose grace already ended is reported before
    /// a new claim starts the next one.
    pub fn observe(
        &mut self,
        store: &str,
        observation: ClaimObservation,
        now: OffsetDateTime,
    ) -> Option<OffsetDateTime> {
        let grace = self.grace;
        let activity = self.stores.entry(store.to_string()).or_default();
        let was_claimed = activity.claims.contains_key(&observation.ticket);
        if observation.completed
            && activity.active_since.is_some()
            && !activity.completed.contains(&observation.slug)
        {
            activity.completed.push(observation.slug.clone());
        }
        match observation.claim {
            Some((worker, expires)) if expires > now => {
                // Starts a period, or continues one whose grace has not ended.
                activity.active_since.get_or_insert(now);
                activity.idle_since = None;
                activity.claims.insert(
                    observation.ticket,
                    Claim {
                        slug: observation.slug,
                        worker,
                        expires,
                    },
                );
            }
            // Released, or written with an already expired lease.
            _ => {
                if was_claimed {
                    activity.claims.remove(&observation.ticket);
                    if activity.claims.is_empty() {
                        activity.idle_since = Some(now);
                    }
                }
            }
        }
        next_check(activity, grace)
    }

    /// Expire overdue leases and report a store that has stopped. Returns the report (at
    /// most once per active period) and the next check time, if any.
    pub fn check(
        &mut self,
        store: &str,
        now: OffsetDateTime,
    ) -> (Option<StopReport>, Option<OffsetDateTime>) {
        let grace = self.grace;
        let Some(activity) = self.stores.get_mut(store) else {
            return (None, None);
        };
        let expired: Vec<String> = activity
            .claims
            .iter()
            .filter(|(_, claim)| claim.expires <= now)
            .map(|(ticket, _)| ticket.clone())
            .collect();
        let mut stalled_now = false;
        for ticket in expired {
            if let Some(claim) = activity.claims.remove(&ticket) {
                stalled_now = true;
                activity.stalled.push((claim.slug, claim.worker));
            }
        }
        if stalled_now && activity.claims.is_empty() {
            // The unrenewed lease was itself the grace period: report the stall now.
            activity.idle_since = Some(now - grace);
        }
        let due = activity.claims.is_empty()
            && activity.idle_since.is_some_and(|idle| idle + grace <= now);
        let report = match (due, activity.active_since) {
            (true, Some(active_since)) => {
                let stalled = std::mem::take(&mut activity.stalled);
                let report = StopReport {
                    reason: if stalled.is_empty() {
                        StopReason::Finished
                    } else {
                        StopReason::Stalled
                    },
                    completed: std::mem::take(&mut activity.completed),
                    stalled,
                    active_since,
                };
                activity.active_since = None;
                activity.idle_since = None;
                Some(report)
            }
            _ => None,
        };
        (report, next_check(activity, grace))
    }

    /// Live claims the tracker holds for a store.
    pub fn active_claims(&self, store: &str) -> usize {
        self.stores.get(store).map_or(0, |a| a.claims.len())
    }
}

fn next_check(activity: &StoreActivity, grace: Duration) -> Option<OffsetDateTime> {
    let lease = activity.claims.values().map(|c| c.expires).min();
    let grace = activity
        .idle_since
        .filter(|_| activity.claims.is_empty() && activity.active_since.is_some())
        .map(|idle| idle + grace);
    match (lease, grace) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::UNIX_EPOCH + Duration::seconds(1_800_000_000 + seconds)
    }

    fn claim(ticket: &str, worker: &str, expires: i64) -> ClaimObservation {
        ClaimObservation {
            ticket: ticket.into(),
            slug: format!("HS2-{ticket}"),
            claim: Some((worker.into(), t(expires))),
            completed: false,
        }
    }

    fn release(ticket: &str, completed: bool) -> ClaimObservation {
        ClaimObservation {
            ticket: ticket.into(),
            slug: format!("HS2-{ticket}"),
            claim: None,
            completed,
        }
    }

    #[test]
    fn release_then_grace_reports_finished_once_with_completed_tickets() {
        let mut p = ProjectActivity::default();
        assert_eq!(p.observe("s", claim("A", "w1", 1800), t(0)), Some(t(1800)));
        let mut done = claim("A", "w1", 1800);
        done.completed = true;
        p.observe("s", done, t(100));
        assert_eq!(p.observe("s", release("A", true), t(110)), Some(t(230)));
        assert_eq!(p.check("s", t(229)).0, None, "still inside the grace");
        let (report, next) = p.check("s", t(230));
        let report = report.unwrap();
        assert_eq!(report.reason, StopReason::Finished);
        assert_eq!(report.completed, ["HS2-A"]);
        assert_eq!(report.active_since, t(0));
        assert_eq!(next, None);
        assert_eq!(
            p.check("s", t(500)).0,
            None,
            "reported once per active period"
        );
    }

    #[test]
    fn a_new_claim_inside_the_grace_continues_the_active_period() {
        let mut p = ProjectActivity::default();
        p.observe("s", claim("A", "w1", 1800), t(0));
        p.observe("s", release("A", true), t(10));
        p.observe("s", claim("B", "w1", 1800), t(60));
        assert_eq!(p.check("s", t(200)).0, None);
        p.observe("s", release("B", true), t(300));
        let report = p.check("s", t(420)).0.unwrap();
        assert_eq!(report.completed, ["HS2-A", "HS2-B"]);
        assert_eq!(report.active_since, t(0));
    }

    #[test]
    fn two_workers_stop_only_when_both_release() {
        let mut p = ProjectActivity::default();
        p.observe("s", claim("A", "w1", 1800), t(0));
        p.observe("s", claim("B", "w2", 1800), t(5));
        assert_eq!(p.observe("s", release("A", false), t(10)), Some(t(1800)));
        assert_eq!(p.check("s", t(500)).0, None);
        assert_eq!(p.active_claims("s"), 1);
        p.observe("s", release("B", false), t(600));
        assert_eq!(p.check("s", t(720)).0.unwrap().reason, StopReason::Finished);
    }

    #[test]
    fn an_unrenewed_lease_reports_stalled_at_expiry_and_renewal_defers_it() {
        let mut p = ProjectActivity::default();
        p.observe("s", claim("A", "w1", 600), t(0));
        assert_eq!(
            p.observe("s", claim("A", "w1", 1200), t(500)),
            Some(t(1200))
        );
        assert_eq!(p.check("s", t(600)), (None, Some(t(1200))));
        let report = p.check("s", t(1200)).0.unwrap();
        assert_eq!(report.reason, StopReason::Stalled);
        assert_eq!(report.stalled, [("HS2-A".to_string(), "w1".to_string())]);
        assert_eq!(p.active_claims("s"), 0);
        // A late release of the expired claim changes nothing.
        assert_eq!(p.observe("s", release("A", false), t(1300)), None);
        assert_eq!(p.check("s", t(2000)).0, None);
    }

    #[test]
    fn one_stalled_claim_waits_for_the_others() {
        let mut p = ProjectActivity::default();
        p.observe("s", claim("A", "w1", 100), t(0));
        p.observe("s", claim("B", "w2", 1800), t(0));
        assert_eq!(p.check("s", t(100)), (None, Some(t(1800))));
        p.observe("s", release("B", false), t(200));
        let report = p.check("s", t(320)).0.unwrap();
        assert_eq!(report.reason, StopReason::Stalled);
        assert_eq!(report.stalled.len(), 1);
    }

    #[test]
    fn empty_then_refill_starts_a_new_period_and_unknown_releases_are_ignored() {
        let mut p = ProjectActivity::default();
        assert_eq!(
            p.observe("s", release("X", true), t(0)),
            None,
            "never claimed"
        );
        assert_eq!(p.check("s", t(1000)).0, None);
        assert_eq!(p.check("other", t(1000)), (None, None));
        p.observe("s", claim("A", "w1", 1800), t(0));
        p.observe("s", release("A", false), t(10));
        assert!(p.check("s", t(130)).0.is_some());
        p.observe("s", claim("B", "w1", 5000), t(2000));
        p.observe("s", release("B", false), t(2100));
        let report = p.check("s", t(2220)).0.unwrap();
        assert_eq!(report.active_since, t(2000));
        assert!(report.completed.is_empty());
        // Repeated releases and an already expired claim write do not start a period.
        p.observe("s", release("B", false), t(2300));
        assert_eq!(p.observe("s", claim("C", "w1", 2300), t(2400)), None);
        assert_eq!(p.check("s", t(9999)).0, None);
    }

    #[test]
    fn stores_are_independent() {
        let mut p = ProjectActivity::default();
        p.observe("a", claim("A", "w1", 1800), t(0));
        p.observe("b", claim("B", "w1", 1800), t(0));
        p.observe("a", release("A", false), t(10));
        assert!(p.check("a", t(130)).0.is_some());
        assert_eq!(p.check("b", t(130)).0, None);
    }
}
