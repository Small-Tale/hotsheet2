//! HS2-EX594V: a multi-site project's home site out of reach, shown and announced.
//!
//! There is no failover (the maintainer's decision on HS2-EX594V). Each scheduling pass
//! observes every multi-site home ([`observe_homes`]); an applied pass folds that into
//! this machine's ledger ([`HomeOutageStore`]) and hands each notification that is due
//! to the host's [`HomeNotices`]. The server publishes them through its notification
//! inbox, so they reach the app and every notifier channel a `notification` rule
//! routes. A process without a host (a headless `scheduler tick`) advances the ledger
//! and leaves the notifications to the server's loop, which sends each one once.

use std::sync::Arc;
use std::time::Duration;

use hotsheet_ticketing::orchestration::{
    HOME_OUTAGE_NOTIFY_AFTER_MS, HomeNotice, HomeNoticeKind, HomeOutage, HomeOutageReason,
    HomeOutageStore, OrchestrationError, ProjectSites, ScheduleInput, advance_outages,
    next_notice_at, observe_homes,
};
use serde::Serialize;

use super::OrchestrationRuntime;

/// One notification a home outage produced, worded for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HomeNotification {
    pub owner_id: String,
    pub home_target_id: String,
    pub kind: HomeNoticeKind,
    /// `warning` for an outage, `success` for its end.
    pub severity: &'static str,
    pub title: String,
    pub body: String,
    /// The same outage is announced once, whoever sends it.
    pub dedupe_key: String,
}

/// Where home-outage notifications go. The server's inbox in a server; nothing
/// elsewhere.
pub trait HomeNotices: Send + Sync {
    fn notify(&self, notification: &HomeNotification);
}

/// The reason, as one sentence.
pub fn reason_sentence(reason: Option<HomeOutageReason>, detail: Option<&str>) -> String {
    let code = detail.map_or_else(String::new, |code| format!(" ({code})"));
    match reason {
        Some(HomeOutageReason::Disabled) => "The orchestration policy disables it.".into(),
        Some(HomeOutageReason::Unbound) => {
            format!("It has no usable binding on the orchestrating machine{code}.")
        }
        Some(HomeOutageReason::PoolUnreadable) => {
            "Its machine answered, but its coordination pool could not be read.".into()
        }
        Some(HomeOutageReason::Unreachable) | None => {
            format!("Its machine cannot be reached{code}.")
        }
    }
}

fn minutes(ms: u64) -> String {
    match ms / 60_000 {
        0 | 1 => "about a minute".into(),
        n => format!("{n} minutes"),
    }
}

fn join(labels: &[String]) -> String {
    match labels {
        [] => "no other site".into(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The notification for `notice`, with targets named by `label`.
pub fn notification(
    owner_id: &str,
    notice: &HomeNotice,
    now_ms: u64,
    label: &dyn Fn(&str) -> String,
) -> HomeNotification {
    let home = label(&notice.home_target_id);
    let sites: Vec<String> = notice.sites.iter().map(|site| label(site)).collect();
    let (severity, title, body, key) = match notice.kind {
        HomeNoticeKind::Unavailable => (
            "warning",
            format!("Home site {home} is unavailable"),
            format!(
                "{} No dispatcher or integrator starts for the project until it is back; \
                 there is no failover to another site. Work placed on {} still runs: each \
                 run waits a bounded time for a dispatcher, then claims its ticket for its \
                 own session. Bring the home's machine back online, or check its binding \
                 and policy in the Orchestration settings. Unavailable for {}.",
                reason_sentence(notice.reason, notice.detail.as_deref()),
                join(&sites),
                minutes(now_ms.saturating_sub(notice.at_ms)),
            ),
            "home-outage",
        ),
        HomeNoticeKind::Returned => (
            "success",
            format!("Home site {home} is back"),
            format!(
                "{home} can coordinate its project again. Its dispatcher and integrator \
                 start as the project's work needs them."
            ),
            "home-returned",
        ),
    };
    HomeNotification {
        owner_id: owner_id.to_owned(),
        home_target_id: notice.home_target_id.clone(),
        kind: notice.kind,
        severity,
        title,
        body,
        dedupe_key: format!(
            "{key}:{owner_id}:{}:{}",
            notice.home_target_id, notice.at_ms
        ),
    }
}

impl OrchestrationRuntime {
    /// HS2-EX594V: send home-outage notifications through `notices`.
    #[must_use]
    pub fn with_home_notices(mut self, notices: Arc<dyn HomeNotices>) -> Self {
        self.home_notices = Some(notices);
        self
    }

    /// HS2-EX594V: how long a home must be unavailable before it is announced (tests
    /// shorten it; [`HOME_OUTAGE_NOTIFY_AFTER_MS`] otherwise).
    #[must_use]
    pub fn with_home_notify_after(mut self, after: Duration) -> Self {
        self.home_notify_after_ms = u64::try_from(after.as_millis()).unwrap_or(u64::MAX);
        self
    }

    /// The owner's current home outages, from this machine's ledger.
    pub fn home_outages(&self, owner_id: &str) -> Result<Vec<HomeOutage>, OrchestrationError> {
        Ok(HomeOutageStore::new(&self.home)
            .outages(owner_id)?
            .into_iter()
            .filter(HomeOutage::current)
            .collect())
    }

    /// One pass's view of the owner's home outages. An applied pass records it and
    /// sends what is due; a dry run changes nothing. Returns the current outages and,
    /// applied with a notifier, when the next notification falls due.
    pub(super) fn home_outage_pass(
        &self,
        owner_id: &str,
        input: &ScheduleInput<'_>,
        applied: bool,
    ) -> (Vec<HomeOutage>, Option<u64>) {
        let bindings = hotsheet_ticketing::orchestration::BindingRegistry::new(&self.home);
        let bound = |target_id: &str| {
            Self::target_ref(owner_id, target_id)
                .ok()
                .and_then(|reference| bindings.get(&reference).ok())
                .and_then(|binding| binding.destination)
                .is_some()
        };
        let observed = observe_homes(input, &bound);
        let store = HomeOutageStore::new(&self.home);
        let sites: &ProjectSites = input.sites;
        let now_ms = input.now_ms;
        let ledger = if applied {
            let notify = self
                .home_notices
                .as_ref()
                .map(|_| self.home_notify_after_ms);
            match store.advance(owner_id, &observed, sites, now_ms, notify) {
                Ok((ledger, notices)) => {
                    if let Some(host) = &self.home_notices
                        && !notices.is_empty()
                    {
                        let labels = self.declarations(owner_id).map(|view| view.targets).ok();
                        let label = |target_id: &str| {
                            labels
                                .as_ref()
                                .and_then(|targets| {
                                    targets.iter().find(|target| target.target_id == target_id)
                                })
                                .map_or_else(|| target_id.to_owned(), |target| target.label.clone())
                        };
                        for notice in &notices {
                            host.notify(&notification(owner_id, notice, now_ms, &label));
                        }
                    }
                    ledger
                }
                // An unreadable ledger hides no placement: the pass goes on without it.
                Err(_) => Vec::new(),
            }
        } else {
            let before = store.outages(owner_id).unwrap_or_default();
            advance_outages(&before, &observed, sites, now_ms)
        };
        let wake = (applied && self.home_notices.is_some())
            .then(|| next_notice_at(&ledger, self.home_notify_after_ms))
            .flatten();
        (
            ledger.into_iter().filter(HomeOutage::current).collect(),
            wake,
        )
    }
}

/// The default notify delay, for the runtime's constructor.
pub(super) const DEFAULT_NOTIFY_AFTER_MS: u64 = HOME_OUTAGE_NOTIFY_AFTER_MS;
