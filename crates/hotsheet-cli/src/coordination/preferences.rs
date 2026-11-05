//! Worker ticket preferences and specific requests (HS2-6WFCZR).
//!
//! A worker states what it would like on its availability session; the assignment
//! authority (the dispatcher, or the integrator in legacy mode) still decides. Strict
//! filters (`only`, `avoid`) are enforced at the reservation boundary. `prefer` is a soft
//! ranking inside the normal priority order. A request names one ticket; the authority
//! honors it only when the ticket passes the same checks as any assignment, and otherwise
//! records an explicit refusal the worker sees on its next wait.
//!
//! Preferences are stored on the worker session, skipped when empty, and require schema
//! nine, so pools without them serialize exactly as before.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use anyhow::{Result, ensure};
use hotsheet_model::{Priority, Ticket};
use hotsheet_ticketing::{TicketRef, ops};
use serde::{Deserialize, Serialize};

use super::contract::{Authority, State, TransitionActor};

/// The schema that first stores worker preferences.
pub const PREFERENCES_SCHEMA: u32 = 9;

/// Longest accepted filter value, request slug and refusal detail, in characters.
pub const MAX_FILTER_CHARS: usize = 128;
pub const MAX_DETAIL_CHARS: usize = 500;
/// Most filters one session may state across `prefer`, `only` and `avoid`.
pub const MAX_FILTERS: usize = 32;
/// Most candidate slugs reported per worker.
pub const MAX_CANDIDATES: usize = 10;

/// Filter kinds reported by `coordination capabilities`.
pub const FILTER_KINDS: &[&str] = &["tag", "category", "priority", "tier"];

/// One ticket filter: `tag:<name>`, `category:<name>`, `priority:<level>` or `tier:<Tn>`.
/// A tier filter matches the `tier:<Tn>` tag. Matching ignores ASCII case.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TicketFilter {
    Tag(String),
    Category(String),
    Priority(PriorityLevel),
    Tier(String),
}

/// A priority filter value; ordered from highest to lowest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PriorityLevel(u8);

impl PriorityLevel {
    const NAMES: [&'static str; 5] = ["highest", "high", "default", "low", "lowest"];

    pub fn of(priority: Priority) -> Self {
        Self(priority_rank(priority))
    }
}

impl fmt::Display for PriorityLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(Self::NAMES[usize::from(self.0)])
    }
}

/// Queue order rank: highest priority first.
pub fn priority_rank(priority: Priority) -> u8 {
    match priority {
        Priority::Highest => 0,
        Priority::High => 1,
        Priority::Default => 2,
        Priority::Low => 3,
        Priority::Lowest => 4,
    }
}

impl TicketFilter {
    pub fn matches(&self, ticket: &Ticket) -> bool {
        match self {
            Self::Tag(tag) => ticket.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)),
            Self::Category(category) => ticket.category.eq_ignore_ascii_case(category),
            Self::Priority(level) => PriorityLevel::of(ticket.priority) == *level,
            Self::Tier(tier) => ticket.tags.iter().any(|t| {
                t.split_once(':').is_some_and(|(kind, value)| {
                    kind.eq_ignore_ascii_case("tier") && value.eq_ignore_ascii_case(tier)
                })
            }),
        }
    }
}

impl fmt::Display for TicketFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tag(value) => write!(f, "tag:{value}"),
            Self::Category(value) => write!(f, "category:{value}"),
            Self::Priority(level) => write!(f, "priority:{level}"),
            Self::Tier(value) => write!(f, "tier:{value}"),
        }
    }
}

impl FromStr for TicketFilter {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let usage = || {
            format!(
                "filter must be tag:<name>, category:<name>, priority:<highest|high|default|low|lowest> or tier:<T0|T1|T2>, got {raw:?}"
            )
        };
        let (kind, value) = raw.trim().split_once(':').ok_or_else(usage)?;
        let value = value.trim();
        if value.is_empty()
            || value.chars().count() > MAX_FILTER_CHARS
            || value.chars().any(char::is_control)
        {
            return Err(usage());
        }
        match kind.trim().to_ascii_lowercase().as_str() {
            "tag" => Ok(Self::Tag(value.into())),
            "category" => Ok(Self::Category(value.to_ascii_lowercase())),
            "priority" => PriorityLevel::NAMES
                .iter()
                .position(|name| name.eq_ignore_ascii_case(value))
                .map(|rank| Self::Priority(PriorityLevel(rank as u8)))
                .ok_or_else(usage),
            "tier" => {
                let tier = value.to_ascii_uppercase();
                let valid = tier.len() == 2
                    && tier.starts_with('T')
                    && tier[1..].bytes().all(|b| b.is_ascii_digit());
                valid.then_some(Self::Tier(tier)).ok_or_else(usage)
            }
            _ => Err(usage()),
        }
    }
}

impl Serialize for TicketFilter {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TicketFilter {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Why the assignment authority did not honor a worker's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum RefusalReason {
    /// Blocked reason, an open dependency, or an integrator block.
    Blocked,
    /// Another live ticket claim holds it.
    Claimed,
    /// Its scope overlaps in-flight or assigned work.
    Overlap,
    /// The ticket's source cannot be verified by this executable.
    UnsupportedSource,
    /// Not in Up Next, closed, or not a ticket of the selected source.
    NotReady,
    /// Already reserved for another worker.
    Assigned,
    /// Outside the worker's own strict `only`/`avoid` filters.
    Filtered,
}

impl fmt::Display for RefusalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Blocked => "blocked",
            Self::Claimed => "claimed",
            Self::Overlap => "overlap",
            Self::UnsupportedSource => "unsupported_source",
            Self::NotReady => "not_ready",
            Self::Assigned => "assigned",
            Self::Filtered => "filtered",
        })
    }
}

/// A durable refusal recorded by the assignment authority for one exact request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestRefusal {
    pub slug: String,
    pub reason: RefusalReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// RFC 3339 instant from the mutation boundary.
    pub at: String,
    pub by: TransitionActor,
}

/// A worker session's stated preferences. Empty for every other role and for workers that
/// state none, in which case the session serializes exactly as before schema nine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerPreferences {
    /// Soft: ranked first within the normal priority order; any eligible ticket remains.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prefer: Vec<TicketFilter>,
    /// Strict: a ticket must match at least one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<TicketFilter>,
    /// Strict: a ticket must match none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub avoid: Vec<TicketFilter>,
    /// One specific ticket slug the worker asks for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    /// The authority's refusal of exactly `request`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<RequestRefusal>,
}

impl WorkerPreferences {
    pub fn is_empty(&self) -> bool {
        self.prefer.is_empty()
            && self.only.is_empty()
            && self.avoid.is_empty()
            && self.request.is_none()
            && self.refusal.is_none()
    }

    /// Validate and normalize a stated set: sorted, deduplicated filters, and a trimmed
    /// request. A refusal is never stated by the worker.
    pub fn stated(
        prefer: Vec<TicketFilter>,
        only: Vec<TicketFilter>,
        avoid: Vec<TicketFilter>,
        request: Option<String>,
    ) -> Result<Self> {
        let normalize = |mut filters: Vec<TicketFilter>| {
            filters.sort();
            filters.dedup();
            filters
        };
        let (prefer, only, avoid) = (normalize(prefer), normalize(only), normalize(avoid));
        ensure!(
            prefer.len() + only.len() + avoid.len() <= MAX_FILTERS,
            "at most {MAX_FILTERS} preference filters"
        );
        ensure!(
            !only.iter().any(|filter| avoid.contains(filter)),
            "a filter cannot be both --only and --avoid"
        );
        let request = request.map(|slug| slug.trim().to_owned());
        if let Some(slug) = &request {
            ensure!(
                !slug.is_empty()
                    && slug.chars().count() <= MAX_FILTER_CHARS
                    && !slug.chars().any(|c| c.is_whitespace() || c.is_control()),
                "request must be one ticket slug"
            );
        }
        Ok(Self {
            prefer,
            only,
            avoid,
            request,
            refusal: None,
        })
    }

    pub fn is_strict(&self) -> bool {
        !self.only.is_empty() || !self.avoid.is_empty()
    }

    /// Whether the strict filters admit this ticket.
    pub fn admits(&self, ticket: &Ticket) -> bool {
        (self.only.is_empty() || self.only.iter().any(|f| f.matches(ticket)))
            && !self.avoid.iter().any(|f| f.matches(ticket))
    }

    pub fn prefers(&self, ticket: &Ticket) -> bool {
        self.prefer.iter().any(|f| f.matches(ticket))
    }

    /// Plain description of the strict filters, for reasons and refusals.
    pub fn strict_summary(&self) -> String {
        let join = |filters: &[TicketFilter]| {
            filters
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" or ")
        };
        let mut parts = Vec::new();
        if !self.only.is_empty() {
            parts.push(format!("only {}", join(&self.only)));
        }
        if !self.avoid.is_empty() {
            parts.push(format!("avoiding {}", join(&self.avoid)));
        }
        parts.join(", ")
    }

    pub(super) fn requests(&self, slug: &str) -> bool {
        self.request
            .as_deref()
            .is_some_and(|request| request.eq_ignore_ascii_case(slug))
    }
}

/// A registration's preference statement (HS2-6WFCZR). `set: None` keeps a same-generation
/// session's preferences, which is how lease renewals and every wait replay them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreferenceRequest {
    pub set: Option<WorkerPreferences>,
    /// Confirms every participant runs a schema-nine artifact before the first preferences.
    pub all_participants_upgraded: bool,
}

impl PreferenceRequest {
    pub fn keep() -> Self {
        Self::default()
    }

    pub fn replace(preferences: WorkerPreferences, all_participants_upgraded: bool) -> Self {
        Self {
            set: Some(preferences),
            all_participants_upgraded,
        }
    }

    pub fn is_keep(&self) -> bool {
        self.set.is_none()
    }
}

/// One ready ticket a worker's strict filters admit, in dispatcher order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub ticket: TicketRef,
    pub slug: String,
    pub priority: Priority,
    pub preferred: bool,
}

/// Facts about a worker's pending request. `reasons` is empty when the ticket is
/// currently eligible apart from scope overlap, which only the authority can judge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestCheck {
    pub slug: String,
    pub ticket: Option<TicketRef>,
    pub reasons: Vec<RefusalReason>,
    pub refusal: Option<RequestRefusal>,
}

/// The dispatcher-facing projection of one worker's preferences against the queue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerMatch {
    pub worker: String,
    /// Ready tickets the strict filters admit: priority first, then preferred, then queue
    /// order. Capped at [`MAX_CANDIDATES`].
    pub candidates: Vec<Candidate>,
    /// Why an idle worker with strict filters has nothing to receive, never silent.
    pub idle_reason: Option<String>,
    pub request: Option<RequestCheck>,
}

/// Ready-ticket facts the snapshot already holds.
pub(super) struct QueueFacts<'a> {
    pub connection_id: &'a str,
    pub tickets: &'a [Ticket],
    pub now: &'a hotsheet_model::Timestamp,
}

impl QueueFacts<'_> {
    fn open_dependency(&self, ticket: &Ticket) -> bool {
        ticket.blocked_by.iter().any(|dependency| {
            !self
                .tickets
                .iter()
                .find(|candidate| candidate.id == *dependency)
                .is_some_and(|candidate| !ops::is_open(candidate))
        })
    }

    fn reference(&self, ticket: &Ticket) -> TicketRef {
        TicketRef {
            connection_id: self.connection_id.to_owned(),
            native_id: ticket.id.to_string(),
        }
    }

    /// Every reason a ticket cannot be reserved for `worker` now, in a fixed order.
    fn reasons(
        &self,
        state: &State,
        ticket: &Ticket,
        worker: &str,
        preferences: &WorkerPreferences,
    ) -> Vec<RefusalReason> {
        let mut reasons = Vec::new();
        if !ticket.up_next || !ops::is_open(ticket) {
            reasons.push(RefusalReason::NotReady);
        }
        let reference = self.reference(ticket);
        let integrator_blocked = state.authority == Authority::Dispatcher
            && state
                .integration
                .as_ref()
                .is_some_and(|report| report.blocked.contains(&reference));
        if ticket
            .blocked_reason
            .as_ref()
            .is_some_and(|r| !r.trim().is_empty())
            || integrator_blocked
            || (state.authority == Authority::Dispatcher && self.open_dependency(ticket))
        {
            reasons.push(RefusalReason::Blocked);
        }
        if !ops::claim_available(ticket, self.now) && ticket.claimed_by.as_deref() != Some(worker) {
            reasons.push(RefusalReason::Claimed);
        }
        if state.assignments.values().any(|a| {
            a.ticket == reference
                && a.phase.pending()
                && !a.returned_for_review()
                && a.worker != worker
        }) {
            reasons.push(RefusalReason::Assigned);
        }
        if !preferences.admits(ticket) {
            reasons.push(RefusalReason::Filtered);
        }
        reasons
    }

    /// Reservable by some worker: no worker id matches "", so every claim and pending
    /// assignment counts against it.
    fn ready(&self, state: &State, ticket: &Ticket) -> bool {
        self.reasons(state, ticket, "", &WorkerPreferences::default())
            .is_empty()
    }

    fn ordered_ready(&self, state: &State) -> Vec<&Ticket> {
        let mut ready: Vec<&Ticket> = self
            .tickets
            .iter()
            .filter(|ticket| self.ready(state, ticket))
            .collect();
        ready.sort_by_key(|ticket| (priority_rank(ticket.priority), ticket.id));
        ready
    }
}

/// Project every worker session that states preferences, plus the ready tickets no live
/// idle worker admits (`unmatched`, the starvation signal). `idle` lists live workers with
/// no active assignment or claim. `queue` is None when the selected source has no local
/// queue, so candidates cannot be computed.
pub(super) fn project(
    state: &State,
    queue: Option<&QueueFacts<'_>>,
    idle: &[String],
) -> (BTreeMap<String, WorkerMatch>, Vec<String>) {
    let mut matches = BTreeMap::new();
    let ready = queue.map(|q| q.ordered_ready(state)).unwrap_or_default();
    for session in state.workers.values() {
        let preferences = &session.preferences;
        if preferences.is_empty() {
            continue;
        }
        let mut candidates: Vec<&Ticket> = ready
            .iter()
            .copied()
            .filter(|ticket| preferences.admits(ticket))
            .collect();
        candidates.sort_by_key(|ticket| {
            (
                priority_rank(ticket.priority),
                !preferences.prefers(ticket),
                ticket.id,
            )
        });
        let idle_reason =
            (idle.contains(&session.id) && preferences.is_strict() && candidates.is_empty())
                .then(|| idle_reason(preferences, queue, state));
        let request = preferences
            .request
            .as_ref()
            .map(|slug| request_check(state, queue, &session.id, preferences, slug));
        let candidates = queue.map_or_else(Vec::new, |q| {
            candidates
                .into_iter()
                .take(MAX_CANDIDATES)
                .map(|ticket| Candidate {
                    ticket: q.reference(ticket),
                    slug: ticket.slug.clone(),
                    priority: ticket.priority,
                    preferred: preferences.prefers(ticket),
                })
                .collect()
        });
        matches.insert(
            session.id.clone(),
            WorkerMatch {
                worker: session.id.clone(),
                candidates,
                idle_reason,
                request,
            },
        );
    }
    let idle_preferences: Vec<&WorkerPreferences> = idle
        .iter()
        .filter_map(|id| state.workers.get(id))
        .map(|session| &session.preferences)
        .collect();
    let unmatched = if idle_preferences.is_empty() {
        Vec::new()
    } else {
        ready
            .iter()
            .filter(|ticket| !idle_preferences.iter().any(|p| p.admits(ticket)))
            .map(|ticket| ticket.slug.clone())
            .collect()
    };
    (matches, unmatched)
}

fn idle_reason(
    preferences: &WorkerPreferences,
    queue: Option<&QueueFacts<'_>>,
    state: &State,
) -> String {
    let summary = preferences.strict_summary();
    let Some(queue) = queue else {
        return format!(
            "no ready tickets ({summary}): the selected source has no local queue to match"
        );
    };
    let waiting: Vec<&str> = queue
        .tickets
        .iter()
        .filter(|ticket| ticket.up_next && ops::is_open(ticket) && preferences.admits(ticket))
        .filter(|ticket| !queue.ready(state, ticket))
        .map(|ticket| ticket.slug.as_str())
        .collect();
    if waiting.is_empty() {
        format!("no ready tickets match {summary}")
    } else {
        let shown = waiting
            .iter()
            .take(3)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        let more = waiting.len().saturating_sub(3);
        format!(
            "no ready tickets match {summary}; {} matching not ready (blocked, claimed or assigned): {shown}{}",
            waiting.len(),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        )
    }
}

fn request_check(
    state: &State,
    queue: Option<&QueueFacts<'_>>,
    worker: &str,
    preferences: &WorkerPreferences,
    slug: &str,
) -> RequestCheck {
    let refusal = preferences
        .refusal
        .clone()
        .filter(|refusal| refusal.slug.eq_ignore_ascii_case(slug));
    let Some(queue) = queue else {
        return RequestCheck {
            slug: slug.to_owned(),
            ticket: None,
            reasons: vec![RefusalReason::UnsupportedSource],
            refusal,
        };
    };
    match queue
        .tickets
        .iter()
        .find(|ticket| ticket.slug.eq_ignore_ascii_case(slug))
    {
        None => RequestCheck {
            slug: slug.to_owned(),
            ticket: None,
            reasons: vec![RefusalReason::NotReady],
            refusal,
        },
        Some(ticket) => RequestCheck {
            slug: slug.to_owned(),
            ticket: Some(queue.reference(ticket)),
            reasons: queue.reasons(state, ticket, worker, preferences),
            refusal,
        },
    }
}
