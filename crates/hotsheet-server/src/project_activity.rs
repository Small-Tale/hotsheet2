//! Per-store AI activity from ticket claims (HS2-2MM07G, `docs/32-notifier-plugins.md`
//! §32.8.1). A store is **active** while any ticket holds a live claim. It has **stopped**
//! when its last claim was released and no claim appeared during the grace period, and it
//! has **stalled** when a claim's lease expired without renewal: the session that held it
//! stopped renewing, so it is likely stuck or gone. It **may be stuck** when claims are held
//! but nothing progressed for the idle window, and a **halted** session is reported for the
//! stores its worker holds claims in (HS2-P57XHV). Pure state; the router supplies the
//! clock and schedules the checks this returns.
//!
//! [`ProjectActivity::live`] reads the same state as a continuous word for clients
//! (HS2-JTTQMY): the one-shot reports above stay the notification path, while the tracker
//! keeps the last period's outcome, recorded halts, per-ticket progress times, and the
//! workers seen in the period, so glance surfaces show the states notifications use.
//!
//! It also keeps each halt's detail and the terminal prompts each worker's session waits at,
//! from which the live state derives its stuck causes (HS2-EGKKA0, §32.8.4).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use time::{Duration, OffsetDateTime};

/// How long a store must stay without claims before it counts as stopped, so a worker
/// moving to its next ticket does not trigger a notification.
pub const IDLE_GRACE: Duration = Duration::seconds(120);

/// How long claims may be held without progress before the store may be stuck.
pub const IDLE_WINDOW: Duration = Duration::minutes(30);

/// How far back per-ticket progress times are kept for "tickets updated per hour".
pub const UPDATED_WINDOW: Duration = Duration::minutes(60);

/// One ticket's claim state as the router observed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimObservation {
    pub ticket: String,
    pub slug: String,
    /// `(worker, lease expiry)` while claimed.
    pub claim: Option<(String, OffsetDateTime)>,
    /// The ticket's status is completed or verified.
    pub completed: bool,
    /// The write changed the ticket's progress (notes, status, phase, title), not only
    /// its claim.
    pub progressed: bool,
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
    /// Claims are held but nothing progressed for the idle window.
    Idle,
    /// A session holding a claim reported a halt.
    Halted,
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
    /// `(slug, worker)` of the claims still held (idle) or held by the halted worker.
    pub held: Vec<(String, String)>,
    /// When progress last happened (idle reports).
    pub last_progress: Option<OffsetDateTime>,
    /// The halt message (halt reports).
    pub detail: Option<String>,
}

/// How the last active period ended, kept until the next period starts (HS2-JTTQMY).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Outcome {
    reason: StopReason,
    at: OffsetDateTime,
    stalled: Vec<(String, String)>,
}

/// A halt a terminal reported for a worker that held claims here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HaltNote {
    pub worker: String,
    pub message: String,
    pub at: OffsetDateTime,
    pub detail: HaltDetail,
}

/// What the router knows about a halt beyond its message (HS2-EGKKA0).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HaltDetail {
    /// The terminal that reported it.
    pub terminal: Option<String>,
    /// The tool reported a usage or rate limit ([`crate::stuck_causes::is_usage_limit`]).
    pub usage_limit: bool,
    /// When the tool said the limit resets; `None` when it did not say so exactly.
    pub resets_at: Option<OffsetDateTime>,
}

/// What a terminal's session is waiting on the user for (HS2-EGKKA0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PromptKind {
    /// A permission request bridged to Hot Sheet.
    Permission,
    /// An `AskUserQuestion` the session presents.
    Question,
}

/// A terminal prompt a worker's session is waiting at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptNote {
    pub worker: String,
    pub terminal: String,
    pub kind: PromptKind,
    /// When the prompt appeared (the hook's time when it reports one, else first seen).
    pub since: OffsetDateTime,
}

/// One prompt as the router reads it from hook state: `(worker, terminal, kind, since)`.
pub type PromptReport = (String, String, PromptKind, Option<OffsetDateTime>);

/// The tracker's continuous state word for one store (HS2-JTTQMY). The words match the
/// notification reasons; `Active` covers the release grace, as notifications do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityWord {
    Active,
    Stopped,
    MayBeStuck,
    Stalled,
    Halted,
}

/// What [`ProjectActivity::live`] reports for a store at a moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveActivity {
    pub word: ActivityWord,
    pub active_since: Option<OffsetDateTime>,
    /// When the last period ended (stopped or stalled); `None` while a period is open.
    pub stopped_at: Option<OffsetDateTime>,
    pub last_progress: Option<OffsetDateTime>,
    /// When the store will count as may-be-stuck without further progress.
    pub idle_deadline: Option<OffsetDateTime>,
    /// `(slug, worker)` of claims whose lease expired without renewal (Stalled only).
    pub stalled: Vec<(String, String)>,
    /// Halts of workers that still hold a live claim here.
    pub halted: Vec<HaltNote>,
    /// Terminal prompts that workers with a live claim here are waiting at (HS2-EGKKA0).
    pub prompts: Vec<PromptNote>,
    /// Distinct workers that held a claim during the current (or stalled) period.
    pub expected_workers: usize,
    /// One time per ticket whose last progress write is inside [`UPDATED_WINDOW`], oldest
    /// first. Claim renewals are not progress.
    pub updated: Vec<OffsetDateTime>,
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
    last_progress: Option<OffsetDateTime>,
    idle_reported: bool,
    outcome: Option<Outcome>,
    halts: BTreeMap<String, HaltNote>,
    /// Last progress write per ticket id, pruned to [`UPDATED_WINDOW`].
    updated: HashMap<String, OffsetDateTime>,
    period_workers: BTreeSet<String>,
}

impl StoreActivity {
    fn holds_live_claim(&self, worker: &str, now: OffsetDateTime) -> bool {
        self.claims
            .values()
            .any(|c| c.worker == worker && c.expires > now)
    }

    /// A halt ends once its worker holds no claim here any more.
    fn drop_orphan_halts(&mut self) {
        let workers: BTreeSet<&str> = self.claims.values().map(|c| c.worker.as_str()).collect();
        self.halts
            .retain(|worker, _| workers.contains(worker.as_str()));
    }
}

#[derive(Debug)]
pub struct ProjectActivity {
    stores: HashMap<String, StoreActivity>,
    /// Open terminal prompts by `(worker, terminal, kind)`, with when each appeared. They
    /// belong to workers, not stores: [`Self::live`] shows them where the worker holds a claim.
    prompts: BTreeMap<(String, String, PromptKind), OffsetDateTime>,
    grace: Duration,
    /// `None` disables idle detection.
    idle: Option<Duration>,
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
            prompts: BTreeMap::new(),
            grace,
            idle: Some(IDLE_WINDOW),
        }
    }

    /// Apply configured windows; `idle: None` disables idle detection.
    pub fn set_windows(&mut self, grace: Duration, idle: Option<Duration>) {
        self.grace = grace;
        self.idle = idle;
    }

    /// Something happened for this store (activity, a turn, a ticket edit): reset idleness.
    pub fn progress(&mut self, store: &str, now: OffsetDateTime) -> Option<OffsetDateTime> {
        let (grace, idle) = (self.grace, self.idle);
        let activity = self.stores.entry(store.to_string()).or_default();
        activity.last_progress = Some(now);
        activity.idle_reported = false;
        next_check(activity, grace, idle)
    }

    /// Reports for every store where `worker` holds a live claim. The router dedupes by
    /// halt episode, so this changes no state.
    pub fn halted(
        &self,
        worker: &str,
        message: &str,
        now: OffsetDateTime,
    ) -> Vec<(String, StopReport)> {
        let mut reports: Vec<(String, StopReport)> = self
            .stores
            .iter()
            .filter_map(|(store, activity)| {
                let held: Vec<(String, String)> = activity
                    .claims
                    .values()
                    .filter(|c| c.worker == worker && c.expires > now)
                    .map(|c| (c.slug.clone(), c.worker.clone()))
                    .collect();
                (!held.is_empty()).then(|| {
                    (
                        store.clone(),
                        StopReport {
                            reason: StopReason::Halted,
                            completed: Vec::new(),
                            stalled: Vec::new(),
                            active_since: activity.active_since.unwrap_or(now),
                            held,
                            last_progress: activity.last_progress,
                            detail: Some(message.to_string()),
                        },
                    )
                })
            })
            .collect();
        reports.sort_by(|a, b| a.0.cmp(&b.0));
        reports
    }

    /// Remember a reported halt for every store where `worker` holds a live claim, so the
    /// live word reads Halted until the session resumes, progresses on its claim, or loses
    /// its claims. Returns the stores that changed.
    pub fn note_halt(&mut self, worker: &str, message: &str, now: OffsetDateTime) -> Vec<String> {
        self.note_halt_with(worker, message, HaltDetail::default(), now)
    }

    /// [`Self::note_halt`] with what else the router knows about the halt (HS2-EGKKA0).
    pub fn note_halt_with(
        &mut self,
        worker: &str,
        message: &str,
        detail: HaltDetail,
        now: OffsetDateTime,
    ) -> Vec<String> {
        let mut changed: Vec<String> = Vec::new();
        for (store, activity) in &mut self.stores {
            if activity.holds_live_claim(worker, now) {
                activity.halts.insert(
                    worker.to_string(),
                    HaltNote {
                        worker: worker.to_string(),
                        message: message.to_string(),
                        at: now,
                        detail: detail.clone(),
                    },
                );
                changed.push(store.clone());
            }
        }
        changed.sort();
        changed
    }

    /// Replace the open terminal prompts with `current`, the complete set the hooks report
    /// now (HS2-EGKKA0). A prompt that stays open keeps its first time unless the hook
    /// reports one. Returns the stores where a worker whose prompts changed holds a claim.
    pub fn sync_prompts(&mut self, current: Vec<PromptReport>, now: OffsetDateTime) -> Vec<String> {
        let mut next: BTreeMap<(String, String, PromptKind), OffsetDateTime> = BTreeMap::new();
        for (worker, terminal, kind, since) in current {
            let key = (worker, terminal, kind);
            let at = since
                .or_else(|| self.prompts.get(&key).copied())
                .unwrap_or(now);
            next.insert(key, at);
        }
        let mut workers: BTreeSet<String> = BTreeSet::new();
        for (key, at) in &next {
            if self.prompts.get(key) != Some(at) {
                workers.insert(key.0.clone());
            }
        }
        for key in self.prompts.keys() {
            if !next.contains_key(key) {
                workers.insert(key.0.clone());
            }
        }
        self.prompts = next;
        let mut changed: Vec<String> = self
            .stores
            .iter()
            .filter(|(_, activity)| {
                activity
                    .claims
                    .values()
                    .any(|claim| workers.contains(&claim.worker))
            })
            .map(|(store, _)| store.clone())
            .collect();
        changed.sort();
        changed
    }

    /// The worker's terminal reported that its session resumed: clear its halts.
    /// Returns the stores that changed.
    pub fn resumed(&mut self, worker: &str) -> Vec<String> {
        let mut changed: Vec<String> = self
            .stores
            .iter_mut()
            .filter_map(|(store, activity)| activity.halts.remove(worker).map(|_| store.clone()))
            .collect();
        changed.sort();
        changed
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
        let (grace, idle) = (self.grace, self.idle);
        let activity = self.stores.entry(store.to_string()).or_default();
        let was_claimed = activity.claims.contains_key(&observation.ticket);
        if observation.progressed {
            activity.last_progress = Some(now);
            activity.idle_reported = false;
            activity.updated.insert(observation.ticket.clone(), now);
            // Progress on its own claim shows a halted session is working again.
            if let Some((worker, _)) = &observation.claim {
                activity.halts.remove(worker);
            }
        }
        activity.updated.retain(|_, at| *at + UPDATED_WINDOW > now);
        if observation.completed
            && activity.active_since.is_some()
            && !activity.completed.contains(&observation.slug)
        {
            activity.completed.push(observation.slug.clone());
        }
        match observation.claim {
            Some((worker, expires)) if expires > now => {
                // Starts a period, or continues one whose grace has not ended.
                if activity.active_since.is_none() {
                    activity.active_since = Some(now);
                    activity.last_progress = Some(now);
                    activity.idle_reported = false;
                    activity.outcome = None;
                    activity.period_workers.clear();
                }
                activity.period_workers.insert(worker.clone());
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
                    activity.drop_orphan_halts();
                    if activity.claims.is_empty() {
                        activity.idle_since = Some(now);
                    }
                }
            }
        }
        next_check(activity, grace, idle)
    }

    /// Expire overdue leases and report a store that has stopped. Returns the report (at
    /// most once per active period) and the next check time, if any.
    pub fn check(
        &mut self,
        store: &str,
        now: OffsetDateTime,
    ) -> (Option<StopReport>, Option<OffsetDateTime>) {
        let (grace, idle) = (self.grace, self.idle);
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
        if stalled_now {
            activity.drop_orphan_halts();
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
                    held: Vec::new(),
                    last_progress: activity.last_progress,
                    detail: None,
                };
                activity.active_since = None;
                activity.idle_since = None;
                activity.outcome = Some(Outcome {
                    reason: report.reason,
                    at: now,
                    stalled: report.stalled.clone(),
                });
                if report.reason == StopReason::Finished {
                    activity.period_workers.clear();
                }
                Some(report)
            }
            _ => idle_report(activity, idle, now),
        };
        (report, next_check(activity, grace, idle))
    }

    /// Live claims the tracker holds for a store.
    pub fn active_claims(&self, store: &str) -> usize {
        self.stores.get(store).map_or(0, |a| a.claims.len())
    }

    /// The idle window in force (`None` when idle detection is off).
    pub fn idle_window(&self) -> Option<Duration> {
        self.idle
    }

    /// The store's continuous live word at `now` (HS2-JTTQMY). Pure: it never reports or
    /// changes state, so the caller re-reads it after each observation or check.
    ///
    /// Precedence: **Halted** (a worker with a live claim reported a halt) over
    /// **Stalled** (no live claim remains and a lease expired unrenewed, until a new claim
    /// starts a period) over **May be stuck** (claims held, no progress for the idle
    /// window) over **Active** (a period is open, including the release grace) over
    /// **Stopped**.
    pub fn live(&self, store: &str, now: OffsetDateTime) -> LiveActivity {
        let Some(activity) = self.stores.get(store) else {
            return LiveActivity {
                word: ActivityWord::Stopped,
                active_since: None,
                stopped_at: None,
                last_progress: None,
                idle_deadline: None,
                stalled: Vec::new(),
                halted: Vec::new(),
                prompts: Vec::new(),
                expected_workers: 0,
                updated: Vec::new(),
            };
        };
        let live: Vec<&Claim> = activity
            .claims
            .values()
            .filter(|c| c.expires > now)
            .collect();
        // Leases that expired but were not checked yet count as stalled already.
        let mut stalled: Vec<(String, String)> = activity.stalled.clone();
        stalled.extend(
            activity
                .claims
                .values()
                .filter(|c| c.expires <= now)
                .map(|c| (c.slug.clone(), c.worker.clone())),
        );
        if stalled.is_empty() && activity.active_since.is_none() {
            if let Some(outcome) = activity
                .outcome
                .as_ref()
                .filter(|o| o.reason == StopReason::Stalled)
            {
                stalled = outcome.stalled.clone();
            }
        }
        let halted: Vec<HaltNote> = activity
            .halts
            .values()
            .filter(|h| live.iter().any(|c| c.worker == h.worker))
            .cloned()
            .collect();
        let mut prompts: Vec<PromptNote> = self
            .prompts
            .iter()
            .filter(|((worker, _, _), _)| live.iter().any(|c| c.worker == *worker))
            .map(|((worker, terminal, kind), since)| PromptNote {
                worker: worker.clone(),
                terminal: terminal.clone(),
                kind: *kind,
                since: *since,
            })
            .collect();
        prompts.sort_by(|a, b| (a.since, &a.worker, a.kind).cmp(&(b.since, &b.worker, b.kind)));
        let idle_deadline = self.idle.filter(|_| !live.is_empty()).and_then(|idle| {
            activity
                .last_progress
                .or(activity.active_since)
                .map(|at| at + idle)
        });
        let word = if !halted.is_empty() {
            ActivityWord::Halted
        } else if live.is_empty() && !stalled.is_empty() {
            ActivityWord::Stalled
        } else if idle_deadline.is_some_and(|deadline| deadline <= now) {
            ActivityWord::MayBeStuck
        } else if activity.active_since.is_some() {
            ActivityWord::Active
        } else {
            ActivityWord::Stopped
        };
        let mut updated: Vec<OffsetDateTime> = activity
            .updated
            .values()
            .copied()
            .filter(|at| *at + UPDATED_WINDOW > now)
            .collect();
        updated.sort();
        LiveActivity {
            word,
            active_since: activity.active_since,
            stopped_at: activity
                .outcome
                .as_ref()
                .filter(|_| activity.active_since.is_none())
                .map(|o| o.at),
            last_progress: activity.last_progress,
            idle_deadline,
            stalled: if word == ActivityWord::Stalled {
                stalled
            } else {
                Vec::new()
            },
            halted,
            prompts,
            expected_workers: match word {
                ActivityWord::Stopped => 0,
                _ => activity.period_workers.len(),
            },
            updated,
        }
    }
}

/// An idle report when claims are held, idle detection is on, the window passed since the
/// last progress, and this idle episode was not reported yet.
fn idle_report(
    activity: &mut StoreActivity,
    idle: Option<Duration>,
    now: OffsetDateTime,
) -> Option<StopReport> {
    let deadline = idle_deadline(activity, idle)?;
    if deadline > now {
        return None;
    }
    activity.idle_reported = true;
    Some(StopReport {
        reason: StopReason::Idle,
        completed: Vec::new(),
        stalled: Vec::new(),
        active_since: activity.active_since.unwrap_or(now),
        held: activity
            .claims
            .values()
            .map(|c| (c.slug.clone(), c.worker.clone()))
            .collect(),
        last_progress: activity.last_progress,
        detail: None,
    })
}

fn idle_deadline(activity: &StoreActivity, idle: Option<Duration>) -> Option<OffsetDateTime> {
    let idle = idle?;
    if activity.claims.is_empty() || activity.idle_reported {
        return None;
    }
    activity
        .last_progress
        .or(activity.active_since)
        .map(|at| at + idle)
}

fn next_check(
    activity: &StoreActivity,
    grace: Duration,
    idle: Option<Duration>,
) -> Option<OffsetDateTime> {
    let lease = activity.claims.values().map(|c| c.expires).min();
    let grace = activity
        .idle_since
        .filter(|_| activity.claims.is_empty() && activity.active_since.is_some())
        .map(|idle| idle + grace);
    [lease, grace, idle_deadline(activity, idle)]
        .into_iter()
        .flatten()
        .min()
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
            progressed: false,
        }
    }

    fn release(ticket: &str, completed: bool) -> ClaimObservation {
        ClaimObservation {
            ticket: ticket.into(),
            slug: format!("HS2-{ticket}"),
            claim: None,
            completed,
            progressed: false,
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
    fn hs2_p57xhv_held_claims_without_progress_report_idle_once_per_episode() {
        let mut p = ProjectActivity::with_grace(IDLE_GRACE);
        p.set_windows(IDLE_GRACE, Some(Duration::minutes(30)));
        // A long lease: only the idle window can fire.
        assert_eq!(
            p.observe("s", claim("A", "w1", 99_999), t(0)),
            Some(t(1800))
        );
        // Renewal alone is not progress.
        assert_eq!(
            p.observe("s", claim("A", "w1", 99_999), t(600)),
            Some(t(1800))
        );
        assert_eq!(p.check("s", t(1799)).0, None);
        let (report, next) = p.check("s", t(1800));
        let report = report.unwrap();
        assert_eq!(report.reason, StopReason::Idle);
        assert_eq!(report.held, [("HS2-A".to_string(), "w1".to_string())]);
        assert_eq!(report.last_progress, Some(t(0)));
        assert_eq!(next, Some(t(99_999)), "reported once; the lease is next");
        assert_eq!(p.check("s", t(5000)).0, None);
        // Progress re-arms it.
        assert_eq!(p.progress("s", t(6000)), Some(t(7800)));
        let mut edited = claim("A", "w1", 99_999);
        edited.progressed = true;
        assert_eq!(p.observe("s", edited, t(7000)), Some(t(8800)));
        assert_eq!(p.check("s", t(8800)).0.unwrap().reason, StopReason::Idle);
        // The period continues: a later release still reports finished.
        p.observe("s", release("A", true), t(9000));
        assert_eq!(
            p.check("s", t(9120)).0.unwrap().reason,
            StopReason::Finished
        );
    }

    #[test]
    fn hs2_p57xhv_idle_detection_can_be_disabled_and_needs_claims() {
        let mut p = ProjectActivity::default();
        p.set_windows(IDLE_GRACE, None);
        assert_eq!(
            p.observe("s", claim("A", "w1", 99_999), t(0)),
            Some(t(99_999))
        );
        assert_eq!(p.check("s", t(50_000)).0, None);
        let mut q = ProjectActivity::default();
        assert_eq!(q.progress("s", t(0)), None, "no claims, nothing to watch");
        assert_eq!(q.check("s", t(99_999)).0, None);
    }

    #[test]
    fn hs2_p57xhv_a_halt_reports_each_store_where_the_worker_holds_a_claim() {
        let mut p = ProjectActivity::default();
        p.observe("a", claim("A", "w1", 1800), t(0));
        p.observe("b", claim("B", "w1", 1800), t(0));
        p.observe("b", claim("C", "w2", 1800), t(0));
        let reports = p.halted("w1", "API overloaded", t(10));
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].0, "a");
        assert_eq!(reports[1].1.held, [("HS2-B".to_string(), "w1".to_string())]);
        assert_eq!(reports[1].1.detail.as_deref(), Some("API overloaded"));
        assert!(p.halted("w3", "x", t(10)).is_empty());
        assert!(
            p.halted("w1", "x", t(2000)).is_empty(),
            "expired claims are not held"
        );
        assert_eq!(p.active_claims("b"), 2, "a halt changes no state");
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

    fn word(p: &ProjectActivity, store: &str, at: i64) -> ActivityWord {
        p.live(store, t(at)).word
    }

    fn progressed(mut observation: ClaimObservation) -> ClaimObservation {
        observation.progressed = true;
        observation
    }

    #[test]
    fn hs2_jttqmy_live_walks_active_idle_progress_grace_and_stopped() {
        let mut p = ProjectActivity::default();
        assert_eq!(word(&p, "s", 0), ActivityWord::Stopped, "never seen");
        p.observe("s", claim("A", "w1", 99_999), t(0));
        let live = p.live("s", t(1));
        assert_eq!(live.word, ActivityWord::Active);
        assert_eq!(live.active_since, Some(t(0)));
        assert_eq!(live.expected_workers, 1);
        assert_eq!(live.idle_deadline, Some(t(1800)));
        // The idle window passes without a check: the word follows the clock.
        assert_eq!(word(&p, "s", 1799), ActivityWord::Active);
        assert_eq!(word(&p, "s", 1800), ActivityWord::MayBeStuck);
        // The one-shot report fires and the word stays until progress.
        assert_eq!(p.check("s", t(1800)).0.unwrap().reason, StopReason::Idle);
        assert_eq!(word(&p, "s", 2500), ActivityWord::MayBeStuck);
        // A renewal is not progress.
        p.observe("s", claim("A", "w1", 99_999), t(2600));
        assert_eq!(word(&p, "s", 2601), ActivityWord::MayBeStuck);
        p.observe("s", progressed(claim("A", "w1", 99_999)), t(2700));
        assert_eq!(word(&p, "s", 2701), ActivityWord::Active);
        assert_eq!(p.live("s", t(2701)).last_progress, Some(t(2700)));
        // Release: the grace still reads Active, as notifications wait for it.
        p.observe("s", release("A", true), t(3000));
        assert_eq!(word(&p, "s", 3001), ActivityWord::Active);
        assert_eq!(p.live("s", t(3001)).idle_deadline, None, "no claims");
        let report = p.check("s", t(3120)).0.unwrap();
        assert_eq!(report.reason, StopReason::Finished);
        let live = p.live("s", t(3121));
        assert_eq!(live.word, ActivityWord::Stopped);
        assert_eq!(live.stopped_at, Some(t(3120)));
        assert_eq!(live.expected_workers, 0);
        assert!(live.stalled.is_empty());
    }

    #[test]
    fn hs2_jttqmy_live_stalled_survives_the_report_until_a_new_period() {
        let mut p = ProjectActivity::default();
        p.observe("s", claim("A", "w1", 600), t(0));
        p.observe("s", claim("B", "w2", 300), t(0));
        assert_eq!(p.live("s", t(10)).expected_workers, 2);
        // One lease lapses while the other is live: still active.
        assert_eq!(word(&p, "s", 300), ActivityWord::Active);
        // Both lapse before any check ran: stalled already.
        let live = p.live("s", t(600));
        assert_eq!(live.word, ActivityWord::Stalled);
        assert_eq!(live.stalled.len(), 2);
        assert_eq!(p.check("s", t(600)).0.unwrap().reason, StopReason::Stalled);
        let live = p.live("s", t(5000));
        assert_eq!(
            live.word,
            ActivityWord::Stalled,
            "survives the one-shot report"
        );
        assert_eq!(live.stopped_at, Some(t(600)));
        assert_eq!(
            live.expected_workers, 2,
            "the stalled workers were expected"
        );
        assert_eq!(
            live.stalled,
            [
                ("HS2-A".to_string(), "w1".to_string()),
                ("HS2-B".to_string(), "w2".to_string())
            ]
        );
        // A late release of an expired claim changes nothing.
        p.observe("s", release("A", false), t(5100));
        assert_eq!(word(&p, "s", 5101), ActivityWord::Stalled);
        // A new claim opens a new period and clears the stall and the old workers.
        p.observe("s", claim("C", "w3", 99_999), t(6000));
        let live = p.live("s", t(6001));
        assert_eq!(live.word, ActivityWord::Active);
        assert!(live.stalled.is_empty());
        assert_eq!(live.stopped_at, None);
        assert_eq!(live.expected_workers, 1);
        // That period finishes cleanly: Stopped, not Stalled.
        p.observe("s", release("C", true), t(6100));
        p.check("s", t(6220));
        assert_eq!(word(&p, "s", 6221), ActivityWord::Stopped);
    }

    #[test]
    fn hs2_jttqmy_live_halt_clears_on_resume_own_progress_or_release() {
        let mut p = ProjectActivity::default();
        p.observe("s", claim("A", "w1", 99_999), t(0));
        p.observe("s", claim("B", "w2", 99_999), t(0));
        p.observe("other", claim("C", "w2", 99_999), t(0));
        assert_eq!(p.note_halt("w1", "API overloaded", t(10)), ["s"]);
        let live = p.live("s", t(11));
        assert_eq!(live.word, ActivityWord::Halted);
        assert_eq!(live.halted[0].message, "API overloaded");
        assert_eq!(word(&p, "other", 11), ActivityWord::Active);
        // Repeating the halt is idempotent; another worker's progress does not clear it.
        assert_eq!(p.note_halt("w1", "API overloaded", t(20)), ["s"]);
        p.observe("s", progressed(claim("B", "w2", 99_999)), t(30));
        assert_eq!(word(&p, "s", 31), ActivityWord::Halted);
        // Halted outranks may-be-stuck.
        assert_eq!(word(&p, "s", 99_000), ActivityWord::Halted);
        // Progress on its own claim clears it.
        p.observe("s", progressed(claim("A", "w1", 99_999)), t(40));
        assert_eq!(word(&p, "s", 41), ActivityWord::Active);
        // A resume clears it.
        p.note_halt("w1", "again", t(50));
        assert_eq!(p.resumed("w1"), ["s"]);
        assert!(p.resumed("w1").is_empty(), "already clear");
        assert_eq!(word(&p, "s", 51), ActivityWord::Active);
        // Losing its claims clears it, and a halt without a live claim records nothing.
        p.note_halt("w1", "third", t(60));
        p.observe("s", release("A", false), t(70));
        assert_eq!(word(&p, "s", 71), ActivityWord::Active);
        assert!(p.note_halt("w1", "late", t(80)).is_empty());
        assert!(p.note_halt("w9", "unknown", t(80)).is_empty());
        // A halted worker's lease lapsing drops the halt; the store then stalls.
        let mut q = ProjectActivity::default();
        q.observe("s", claim("A", "w1", 100), t(0));
        q.note_halt("w1", "x", t(10));
        assert_eq!(word(&q, "s", 99), ActivityWord::Halted);
        assert_eq!(word(&q, "s", 100), ActivityWord::Stalled);
        q.check("s", t(100));
        assert_eq!(word(&q, "s", 101), ActivityWord::Stalled);
        assert!(q.live("s", t(101)).halted.is_empty());
    }

    #[test]
    fn hs2_jttqmy_updated_counts_distinct_progressed_tickets_in_the_last_hour() {
        let mut p = ProjectActivity::default();
        p.observe("s", progressed(release("A", false)), t(0));
        p.observe("s", progressed(release("B", false)), t(100));
        // The same ticket again counts once, at its latest time.
        p.observe("s", progressed(release("A", false)), t(200));
        // A claim and its renewal are not progress.
        p.observe("s", claim("C", "w1", 99_999), t(300));
        p.observe("s", claim("C", "w1", 99_999), t(400));
        assert_eq!(p.live("s", t(500)).updated, [t(100), t(200)]);
        // B ages out of the 60-minute window first.
        assert_eq!(p.live("s", t(3700)).updated, [t(200)]);
        assert!(p.live("s", t(3800)).updated.is_empty());
        // A later write prunes the stored map, then refills it.
        p.observe("s", progressed(release("D", false)), t(9000));
        assert_eq!(p.live("s", t(9001)).updated, [t(9000)]);
    }

    #[test]
    fn hs2_jttqmy_live_without_idle_detection_never_reads_may_be_stuck() {
        let mut p = ProjectActivity::default();
        p.set_windows(IDLE_GRACE, None);
        assert_eq!(p.idle_window(), None);
        p.observe("s", claim("A", "w1", 99_999), t(0));
        assert_eq!(word(&p, "s", 90_000), ActivityWord::Active);
        assert_eq!(p.live("s", t(1)).idle_deadline, None);
    }

    #[test]
    fn hs2_jttqmy_live_empty_then_refill_repeats_the_cycle() {
        let mut p = ProjectActivity::with_grace(Duration::seconds(10));
        for round in 0..3 {
            let base = round * 1000;
            p.observe("s", claim("A", "w1", base + 500), t(base));
            assert_eq!(word(&p, "s", base + 1), ActivityWord::Active);
            p.observe("s", release("A", true), t(base + 50));
            p.observe("s", release("A", true), t(base + 51));
            p.check("s", t(base + 60));
            assert_eq!(word(&p, "s", base + 61), ActivityWord::Stopped);
        }
    }

    fn prompt(worker: &str, terminal: &str, kind: PromptKind, since: Option<i64>) -> PromptReport {
        (worker.into(), terminal.into(), kind, since.map(t))
    }

    fn prompts(p: &ProjectActivity, store: &str, at: i64) -> Vec<(String, PromptKind, i64)> {
        p.live(store, t(at))
            .prompts
            .into_iter()
            .map(|n| (n.worker, n.kind, (n.since - t(0)).whole_seconds()))
            .collect()
    }

    #[test]
    fn hs2_egkka0_prompts_follow_claims_resolution_and_repeated_syncs() {
        let mut p = ProjectActivity::default();
        // Empty, then a prompt for a worker with no claim anywhere: nothing to touch or show.
        assert!(p.sync_prompts(Vec::new(), t(0)).is_empty());
        assert!(
            p.sync_prompts(vec![prompt("w1", "t1", PromptKind::Permission, None)], t(5))
                .is_empty()
        );
        assert!(prompts(&p, "s", 6).is_empty(), "no store knows w1 yet");
        // The worker claims afterwards: the open prompt shows with its first-seen time.
        p.observe("s", claim("A", "w1", 99_999), t(10));
        p.observe("other", claim("B", "w2", 99_999), t(10));
        assert_eq!(
            prompts(&p, "s", 11),
            [("w1".into(), PromptKind::Permission, 5)]
        );
        assert!(
            prompts(&p, "other", 11).is_empty(),
            "another worker's store"
        );
        // Repeating the same set keeps the time and touches nothing.
        let same = vec![prompt("w1", "t1", PromptKind::Permission, None)];
        assert!(p.sync_prompts(same.clone(), t(20)).is_empty());
        assert!(p.sync_prompts(same, t(30)).is_empty());
        assert_eq!(prompts(&p, "s", 31)[0].2, 5);
        // A question joins it (with the hook's own time); both list, oldest first.
        let both = vec![
            prompt("w1", "t1", PromptKind::Question, Some(25)),
            prompt("w1", "t1", PromptKind::Permission, None),
        ];
        assert_eq!(p.sync_prompts(both, t(40)), ["s"]);
        assert_eq!(
            prompts(&p, "s", 41),
            [
                ("w1".into(), PromptKind::Permission, 5),
                ("w1".into(), PromptKind::Question, 25)
            ]
        );
        // A replacement question (new hook time) is a change.
        let replaced = vec![
            prompt("w1", "t1", PromptKind::Question, Some(45)),
            prompt("w1", "t1", PromptKind::Permission, None),
        ];
        assert_eq!(p.sync_prompts(replaced, t(46)), ["s"]);
        // The permission resolves; then a different worker's prompt appears (interleaved).
        let question = vec![prompt("w1", "t1", PromptKind::Question, Some(45))];
        assert_eq!(p.sync_prompts(question.clone(), t(50)), ["s"]);
        let mut with_w2 = question;
        with_w2.push(prompt("w2", "t2", PromptKind::Permission, None));
        assert_eq!(p.sync_prompts(with_w2, t(55)), ["other"]);
        assert_eq!(
            prompts(&p, "other", 56),
            [("w2".into(), PromptKind::Permission, 55)]
        );
        // Prompts are not progress and do not change the tracker's word.
        assert_eq!(word(&p, "s", 56), ActivityWord::Active);
        assert_eq!(word(&p, "s", 99_000), ActivityWord::MayBeStuck);
        // Everything answered: both stores change, and an empty refill starts fresh.
        assert_eq!(p.sync_prompts(Vec::new(), t(60)), ["other", "s"]);
        assert!(prompts(&p, "s", 61).is_empty());
        p.sync_prompts(
            vec![prompt("w1", "t1", PromptKind::Permission, None)],
            t(70),
        );
        assert_eq!(
            prompts(&p, "s", 71)[0].2,
            70,
            "a reopened prompt has a new time"
        );
        // Losing the claim hides the prompt; the lease lapsing hides it too.
        p.observe("s", release("A", false), t(80));
        assert!(prompts(&p, "s", 81).is_empty());
        let mut q = ProjectActivity::default();
        q.observe("s", claim("A", "w1", 100), t(0));
        q.sync_prompts(vec![prompt("w1", "t1", PromptKind::Question, None)], t(1));
        assert_eq!(prompts(&q, "s", 99).len(), 1);
        assert!(prompts(&q, "s", 100).is_empty());
    }

    #[test]
    fn hs2_egkka0_halt_detail_follows_the_halt_lifecycle() {
        let mut p = ProjectActivity::default();
        p.observe("s", claim("A", "w1", 99_999), t(0));
        let limit = HaltDetail {
            terminal: Some("t1".into()),
            usage_limit: true,
            resets_at: Some(t(3600)),
        };
        assert_eq!(p.note_halt_with("w1", "limit", limit.clone(), t(10)), ["s"]);
        let live = p.live("s", t(11));
        assert_eq!(live.word, ActivityWord::Halted);
        assert_eq!(live.halted[0].detail, limit);
        // A later, different halt replaces the detail (an overload is not a pause).
        p.note_halt("w1", "overloaded", t(20));
        assert_eq!(p.live("s", t(21)).halted[0].detail, HaltDetail::default());
        // A reset time passing changes nothing by itself: the session still waits.
        p.note_halt_with("w1", "limit", limit.clone(), t(30));
        assert_eq!(p.live("s", t(7200)).halted[0].detail, limit);
        // Resume clears it, as for any halt.
        assert_eq!(p.resumed("w1"), ["s"]);
        assert!(p.live("s", t(7201)).halted.is_empty());
    }
}
