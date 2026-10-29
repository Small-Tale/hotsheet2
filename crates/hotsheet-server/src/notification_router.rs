//! Server side of notifier plugins (HS2-R5HTZN, `docs/32-notifier-plugins.md` §32.8).
//! One task subscribes to the change bus, turns ticket transitions, permission requests,
//! and published notifications into [`Message`]s, and hands each to the dispatcher on a
//! blocking worker. Every writer already publishes on the bus (server routes and the
//! store watcher for CLI or MCP writes), so this needs no hook in any write path.
//!
//! The bus carries ids, not before/after states, so status, feedback, and blocked
//! transitions are found by comparing each changed ticket with a snapshot seeded from the
//! index when the router starts.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use hotsheet_model::{Ticket, Ulid};
use hotsheet_notify::dispatch::Cooldowns;
use hotsheet_notify::{
    DeliveryRecord, Dispatcher, EventKind, Message, NotificationConfig, Notifier, Outcome,
    Registry, Severity, TicketFacts,
};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::ops::TicketQuery;
use time::OffsetDateTime;
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;
use crate::events::ChangeEvent;
use crate::project_activity::{ClaimObservation, ProjectActivity, StopReason, StopReport};

/// Change kinds that mean a ticket file was written.
const TICKET_KINDS: [&str; 8] = [
    "created", "updated", "changed", "closed", "assigned", "claimed", "released", "moved",
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    status: String,
    feedback_needed: bool,
    blocked: bool,
}

#[derive(Default)]
struct Snapshots {
    seeded: HashSet<String>,
    tickets: HashMap<(String, String), Snapshot>,
}

/// Router state shared by every clone of [`AppState`].
#[derive(Clone, Default)]
pub(crate) struct NotificationDelivery {
    /// Test override for the built-in notifiers; `None` uses the live ones.
    builtins: Option<Arc<Vec<Arc<dyn Notifier>>>>,
    cooldowns: Cooldowns,
    snapshots: Arc<Mutex<Snapshots>>,
    /// Claim-derived project activity for `project.stopped` (HS2-2MM07G).
    activity: Arc<Mutex<ProjectActivity>>,
    activity_seeded: Arc<Mutex<HashSet<String>>>,
    /// The earliest pending activity check per store, so each store has one timer.
    activity_timers: Arc<Mutex<HashMap<String, OffsetDateTime>>>,
}

impl AppState {
    /// Replace the built-in notifiers (tests inject fake transports). External plugins
    /// still load from the machine home.
    pub fn with_notifiers(mut self, notifiers: Vec<Arc<dyn Notifier>>) -> Self {
        self.notification_delivery.builtins = Some(Arc::new(notifiers));
        self
    }

    /// Shorten the idle grace before a project counts as stopped (tests).
    pub fn with_project_idle_grace(mut self, grace: std::time::Duration) -> Self {
        self.notification_delivery.activity = Arc::new(Mutex::new(ProjectActivity::with_grace(
            time::Duration::try_from(grace).unwrap_or(crate::project_activity::IDLE_GRACE),
        )));
        self
    }

    pub(crate) fn notifier_registry(&self) -> Registry {
        match &self.notification_delivery.builtins {
            Some(builtins) => Registry::with_builtins(
                builtins.as_ref().clone(),
                Some(&Registry::plugins_dir(self.machine_home())),
                Arc::new(hotsheet_notify::notifier::LiveRunner),
            ),
            None => Registry::live(self.machine_home()),
        }
    }

    pub(crate) fn notification_dispatcher(&self) -> Dispatcher {
        Dispatcher::new(
            Arc::new(self.notifier_registry()),
            Arc::new(self.key_registry()),
            Some(self.machine_home()),
        )
        .with_cooldowns(self.notification_delivery.cooldowns.clone())
    }
}

/// Start the router: seed ticket snapshots off the runtime (subscribed first, so no
/// change is lost meanwhile), then route each bus event on a blocking worker so a slow
/// channel never delays the bus. Returns once seeding finished.
pub async fn spawn_notification_router(state: AppState) -> tokio::task::JoinHandle<()> {
    let mut events = state.events.subscribe();
    let seeding = state.clone();
    let seed = move || {
        // Without a ticket rule there is nothing to compare; a store is seeded on its
        // first change once such a rule exists.
        let config = NotificationConfig::load(seeding.machine_home()).unwrap_or_default();
        if wants(&config, is_ticket_event) {
            seed_all(&seeding);
        }
        if wants(&config, |kind| *kind == EventKind::ProjectStopped) {
            for (store, _) in seeding.host.locations() {
                seed_activity(&seeding, &store);
            }
        }
    };
    if let Err(error) = tokio::task::spawn_blocking(seed).await {
        tracing::warn!(%error, "notification snapshot seeding failed");
    }
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let state = state.clone();
                    tokio::task::spawn_blocking(move || route_change_event(&state, &event));
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "notification router fell behind the change bus");
                }
                Err(RecvError::Closed) => break,
            }
        }
    })
}

fn wants(config: &NotificationConfig, kind: impl Fn(&EventKind) -> bool) -> bool {
    config
        .rules
        .iter()
        .any(|r| r.enabled && r.events.iter().any(&kind))
}

fn is_ticket_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::TicketStatusChanged | EventKind::TicketFeedbackNeeded | EventKind::TicketBlocked
    )
}

/// Seed snapshots for every hosted store from its index.
pub fn seed_all(state: &AppState) {
    for (store, _) in state.host.locations() {
        seed_store(state, &store);
    }
}

fn seed_store(state: &AppState, store: &str) {
    let Some(entry) = state.host.get(store) else {
        return;
    };
    let rows = entry
        .index
        .with_lock(|index| index.query(&TicketQuery::default()));
    let rows = match rows {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(store, %error, "could not seed notification snapshots");
            return;
        }
    };
    let mut snapshots = state.notification_delivery.snapshots.lock_or_recover();
    for row in rows {
        snapshots.tickets.insert(
            (store.to_string(), row.id.clone()),
            Snapshot {
                status: row.status.unwrap_or_default(),
                feedback_needed: row.feedback_needed,
                blocked: row.blocked_reason.is_some_and(|r| !r.trim().is_empty())
                    || !row.blocked_by.is_empty(),
            },
        );
    }
    snapshots.seeded.insert(store.to_string());
}

/// Route one bus event to the configured channels. Returns the delivery records (empty
/// when nothing matched). Public so tests drive the router without a runtime.
pub fn route_change_event(state: &AppState, event: &ChangeEvent) -> Vec<DeliveryRecord> {
    let home = state.machine_home();
    let config = match NotificationConfig::load(home) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(%error, "notification config is unreadable; nothing was routed");
            return Vec::new();
        }
    };
    if config.rules.iter().all(|rule| !rule.enabled) {
        return Vec::new();
    }
    let wants = |kinds: &[EventKind]| {
        config
            .rules
            .iter()
            .any(|r| r.enabled && r.events.iter().any(|e| kinds.contains(e)))
    };
    let messages = match event.kind.as_str() {
        "permission_asked" if wants(&[EventKind::PermissionRequested]) => {
            vec![permission_message(event)]
        }
        "notification" if wants(&[EventKind::Notification]) => {
            hub_message(state, event).into_iter().collect()
        }
        kind if TICKET_KINDS.contains(&kind) => {
            let mut messages = Vec::new();
            if wants(&[EventKind::ProjectStopped]) {
                messages.extend(observe_activity(state, event));
            }
            if wants(&[
                EventKind::TicketStatusChanged,
                EventKind::TicketFeedbackNeeded,
                EventKind::TicketBlocked,
            ]) {
                messages.extend(ticket_messages(state, event));
            }
            messages
        }
        _ => Vec::new(),
    };
    deliver(state, &config, messages)
}

fn deliver(
    state: &AppState,
    config: &NotificationConfig,
    messages: Vec<Message>,
) -> Vec<DeliveryRecord> {
    if messages.is_empty() {
        return Vec::new();
    }
    let dispatcher = state.notification_dispatcher();
    let mut records = Vec::new();
    for message in messages {
        for record in dispatcher.dispatch(config, &message) {
            if record.outcome == Outcome::Failed {
                tracing::warn!(
                    channel = record.channel,
                    event = record.event.as_str(),
                    error = record.error.as_deref().unwrap_or_default(),
                    "notification delivery failed"
                );
            }
            records.push(record);
        }
    }
    records
}

fn project_name(project: &str) -> String {
    std::path::Path::new(project)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.trim_end_matches(".hs2").to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| project.to_string())
}

fn permission_message(event: &ChangeEvent) -> Message {
    let name = project_name(&event.store);
    let tool = if event.slug.is_empty() {
        "An AI tool"
    } else {
        &event.slug
    };
    let mut message = Message::new(
        EventKind::PermissionRequested,
        format!("{tool} needs permission in {name}"),
    );
    message.body =
        format!("**{tool}** is waiting for a permission decision in Hot Sheet project **{name}**.");
    message.severity = Severity::Warning;
    message.project = Some(event.store.clone());
    message.project_name = Some(name);
    message.dedupe_key = Some(format!("permission:{}", event.id));
    message
}

fn hub_message(state: &AppState, event: &ChangeEvent) -> Option<Message> {
    let published = state
        .notifications
        .list(None, None, None, None)
        .into_iter()
        .find(|n| n.id == event.id)?;
    let mut lines = published.message.lines();
    let title = lines.next().unwrap_or_default().trim().to_string();
    let mut message = Message::new(EventKind::Notification, title);
    message.body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    message.severity = Severity::parse_lenient(&published.severity);
    message.project = published.checkout.clone();
    message.project_name = published.checkout.as_deref().map(project_name);
    message.store = published.store.clone();
    message.ticket = published.ticket.map(|slug| TicketFacts {
        slug,
        ..TicketFacts::default()
    });
    message.dedupe_key = published.dedupe_key;
    Some(message)
}

fn wire(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn label(status: &str) -> String {
    status
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn snapshot_of(ticket: &Ticket) -> Snapshot {
    Snapshot {
        status: wire(ticket.status),
        feedback_needed: ticket.feedback_needed(),
        blocked: ticket
            .blocked_reason
            .as_deref()
            .is_some_and(|r| !r.trim().is_empty())
            || !ticket.blocked_by.is_empty(),
    }
}

fn ticket_messages(state: &AppState, event: &ChangeEvent) -> Vec<Message> {
    let Ok(id) = Ulid::from_string(&event.id) else {
        return Vec::new();
    };
    let Some(entry) = state.host.get(&event.store) else {
        return Vec::new();
    };
    let Ok(ticket) = entry.store.read_ticket(&id) else {
        return Vec::new();
    };
    let current = snapshot_of(&ticket);
    let previous = {
        let mut snapshots = state.notification_delivery.snapshots.lock_or_recover();
        if !snapshots.seeded.contains(&event.store) {
            // A store hosted after startup: its first change only records the baseline.
            drop(snapshots);
            seed_store(state, &event.store);
            snapshots = state.notification_delivery.snapshots.lock_or_recover();
        }
        snapshots
            .tickets
            .insert((event.store.clone(), event.id.clone()), current.clone())
    };
    let created = event.kind == "created";
    let Some(previous) = previous.or_else(|| {
        created.then(|| Snapshot {
            status: current.status.clone(),
            feedback_needed: false,
            blocked: false,
        })
    }) else {
        return Vec::new();
    };
    let facts = TicketFacts {
        slug: ticket.slug.clone(),
        status: Some(current.status.clone()),
        previous_status: Some(previous.status.clone()),
        category: Some(ticket.category.clone()),
        priority: Some(wire(ticket.priority)),
        tags: ticket.tags.clone(),
    };
    let base = |event_kind: EventKind, title: String, severity: Severity, key: String| {
        let mut message = Message::new(event_kind, title);
        message.body = format!("**{}**", escape(&ticket.title));
        message.severity = severity;
        message.project = Some(event.store.clone());
        message.project_name = Some(project_name(&entry.store.root().display().to_string()));
        message.store = Some(event.store.clone());
        message.ticket = Some(facts.clone());
        message.dedupe_key = Some(key);
        message
    };
    let mut messages = Vec::new();
    if previous.status != current.status {
        let mut message = base(
            EventKind::TicketStatusChanged,
            format!("{} is now {}", ticket.slug, label(&current.status)),
            Severity::Info,
            format!("status:{}:{}:{}", event.store, event.id, current.status),
        );
        message.body.push_str(&format!(
            "\n\n{} → {}",
            label(&previous.status),
            label(&current.status)
        ));
        messages.push(message);
    }
    if current.feedback_needed && !previous.feedback_needed {
        messages.push(base(
            EventKind::TicketFeedbackNeeded,
            format!("{} needs your feedback", ticket.slug),
            Severity::Warning,
            format!("feedback:{}:{}", event.store, event.id),
        ));
    }
    if current.blocked && !previous.blocked {
        let mut message = base(
            EventKind::TicketBlocked,
            format!("{} is blocked", ticket.slug),
            Severity::Warning,
            format!("blocked:{}:{}", event.store, event.id),
        );
        if let Some(reason) = ticket
            .blocked_reason
            .as_deref()
            .filter(|r| !r.trim().is_empty())
        {
            message
                .body
                .push_str(&format!("\n\n{}", escape(reason.trim())));
        }
        messages.push(message);
    }
    messages
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn earliest(a: Option<OffsetDateTime>, b: Option<OffsetDateTime>) -> Option<OffsetDateTime> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Record live claims from a store's index as the activity baseline (HS2-2MM07G).
fn seed_activity(state: &AppState, store: &str) {
    let Some(entry) = state.host.get(store) else {
        return;
    };
    let rows = match entry
        .index
        .with_lock(|index| index.query(&TicketQuery::default()))
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(store, %error, "could not seed project activity");
            return;
        }
    };
    let now = now();
    let mut next = None;
    {
        let mut activity = state.notification_delivery.activity.lock_or_recover();
        for row in rows {
            let claim = row.claimed_by.zip(
                row.claim_lease_expires_at
                    .as_deref()
                    .and_then(|at| hotsheet_model::Timestamp::new(at).instant()),
            );
            if claim.is_none() {
                continue;
            }
            let observation = ClaimObservation {
                ticket: row.id,
                slug: row.slug,
                claim,
                completed: false,
            };
            next = earliest(next, activity.observe(store, observation, now));
        }
    }
    state
        .notification_delivery
        .activity_seeded
        .lock_or_recover()
        .insert(store.to_string());
    if let Some(at) = next {
        schedule_activity_check(state, store, at);
    }
}

/// Feed one changed ticket's claim to the activity tracker. A period whose grace already
/// ended is reported first, so a late claim never swallows a stop.
fn observe_activity(state: &AppState, event: &ChangeEvent) -> Vec<Message> {
    let Ok(id) = Ulid::from_string(&event.id) else {
        return Vec::new();
    };
    let Some(entry) = state.host.get(&event.store) else {
        return Vec::new();
    };
    let Ok(ticket) = entry.store.read_ticket(&id) else {
        return Vec::new();
    };
    let seeded = state
        .notification_delivery
        .activity_seeded
        .lock_or_recover()
        .contains(&event.store);
    if !seeded {
        // The index already holds this change, so the seed is the baseline.
        seed_activity(state, &event.store);
    }
    let now = now();
    let claim = ticket.claimed_by.clone().zip(
        ticket
            .claim_lease_expires_at
            .as_ref()
            .and_then(hotsheet_model::Timestamp::instant),
    );
    let observation = ClaimObservation {
        ticket: event.id.clone(),
        slug: ticket.slug.clone(),
        claim,
        completed: matches!(
            ticket.status,
            hotsheet_model::Status::Completed | hotsheet_model::Status::Verified
        ),
    };
    let (report, next) = {
        let mut activity = state.notification_delivery.activity.lock_or_recover();
        let (report, _) = activity.check(&event.store, now);
        let next = activity.observe(&event.store, observation, now);
        (report, next)
    };
    if let Some(at) = next {
        schedule_activity_check(state, &event.store, at);
    }
    report
        .map(|report| stop_message(state, &event.store, &report))
        .into_iter()
        .collect()
}

/// Arm one event-driven timer per store for its earliest pending deadline. No timer is
/// armed outside a Tokio runtime (synchronous tests call [`check_project_activity`]).
fn schedule_activity_check(state: &AppState, store: &str, at: OffsetDateTime) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    {
        let mut timers = state
            .notification_delivery
            .activity_timers
            .lock_or_recover();
        if timers.get(store).is_some_and(|pending| *pending <= at) {
            return;
        }
        timers.insert(store.to_string(), at);
    }
    let delay = std::time::Duration::try_from(at - now()).unwrap_or_default();
    let state = state.clone();
    let store = store.to_string();
    runtime.spawn(async move {
        tokio::time::sleep(delay).await;
        let fired = {
            let mut timers = state
                .notification_delivery
                .activity_timers
                .lock_or_recover();
            // A later observation may have armed an earlier timer; only the owner fires.
            if timers.get(&store) == Some(&at) {
                timers.remove(&store);
                true
            } else {
                false
            }
        };
        if fired {
            let _ =
                tokio::task::spawn_blocking(move || check_project_activity(&state, &store, now()))
                    .await;
        }
    });
}

/// Run a store's activity check at `now`, deliver a stop report, and arm the next
/// deadline. Public so tests drive the clock directly.
pub fn check_project_activity(
    state: &AppState,
    store: &str,
    now: OffsetDateTime,
) -> Vec<DeliveryRecord> {
    let (report, next) = state
        .notification_delivery
        .activity
        .lock_or_recover()
        .check(store, now);
    if let Some(at) = next {
        schedule_activity_check(state, store, at);
    }
    let Some(report) = report else {
        return Vec::new();
    };
    let config = match NotificationConfig::load(state.machine_home()) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(%error, "notification config is unreadable; project stop not routed");
            return Vec::new();
        }
    };
    let message = stop_message(state, store, &report);
    deliver(state, &config, vec![message])
}

fn stop_message(state: &AppState, store: &str, report: &StopReport) -> Message {
    let name = state
        .host
        .get(store)
        .map(|entry| project_name(&entry.store.root().display().to_string()))
        .unwrap_or_else(|| project_name(store));
    let (title, severity) = match report.reason {
        StopReason::Finished => (
            format!("Hot Sheet project {name} has stopped working on tickets"),
            Severity::Warning,
        ),
        StopReason::Stalled => (
            format!("Hot Sheet project {name} has stalled"),
            Severity::Error,
        ),
    };
    let mut body = match report.reason {
        StopReason::Finished => "No AI session holds a ticket claim any more.".to_string(),
        StopReason::Stalled => {
            let claims: Vec<String> = report
                .stalled
                .iter()
                .map(|(slug, worker)| format!("**{}** (worker {})", escape(slug), escape(worker)))
                .collect();
            format!(
                "The claim on {} expired without renewal, so its session may be stuck or gone.",
                claims.join(", ")
            )
        }
    };
    if report.completed.is_empty() {
        body.push_str("\n\nNo tickets were completed in this session.");
    } else {
        let slugs: Vec<String> = report.completed.iter().map(|s| escape(s)).collect();
        body.push_str(&format!("\n\nCompleted: {}.", slugs.join(", ")));
    }
    let mut message = Message::new(EventKind::ProjectStopped, title);
    message.body = body;
    message.severity = severity;
    message.project = Some(store.to_string());
    message.project_name = Some(name);
    message.store = Some(store.to_string());
    message.dedupe_key = Some(format!(
        "project-stopped:{store}:{}",
        report.active_since.unix_timestamp()
    ));
    message
}

/// Ticket text is quoted into Markdown; escape the characters that would restyle it.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '_' | '`' | '[' | ']' | '<' | '>' | '\\' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_project_names_read_naturally() {
        assert_eq!(label("not_started"), "Not Started");
        assert_eq!(label("completed"), "Completed");
        assert_eq!(project_name("/work/hotsheet2.hs2"), "hotsheet2");
        assert_eq!(project_name("plain"), "plain");
        assert_eq!(escape("a*b_[c]"), "a\\*b\\_\\[c\\]");
    }
}
