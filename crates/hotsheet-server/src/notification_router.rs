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
use hotsheet_notify::transitions::{Origin, TicketFlags, TicketState, escape, transition_messages};
use hotsheet_notify::{
    DeliveryRecord, Dispatcher, EventKind, Message, NotificationConfig, Notifier, Outcome,
    Registry, Severity, TicketFacts,
};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::checkouts::Checkout;
use hotsheet_ticketing::ops::TicketQuery;
use time::OffsetDateTime;
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;
use crate::events::ChangeEvent;
use crate::project_activity::{ClaimObservation, ProjectActivity, StopReason, StopReport};

/// Change kinds that mean a ticket file was written.
/// `renewed` moves a lease expiry; without it a renewed claim would read as stalled.
const TICKET_KINDS: [&str; 9] = [
    "created", "updated", "changed", "closed", "assigned", "claimed", "renewed", "released",
    "moved",
];

#[derive(Default)]
struct Snapshots {
    seeded: HashSet<String>,
    tickets: HashMap<(String, String), TicketFlags>,
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
    /// Test override of the release grace; configuration applies otherwise.
    grace_override: Option<time::Duration>,
    /// Last progress fingerprint per `(store, ticket)`, so a claim renewal is not progress.
    fingerprints: Arc<Mutex<HashMap<(String, String), u64>>>,
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
        let grace = time::Duration::try_from(grace).unwrap_or(crate::project_activity::IDLE_GRACE);
        self.notification_delivery.grace_override = Some(grace);
        self.notification_delivery.activity =
            Arc::new(Mutex::new(ProjectActivity::with_grace(grace)));
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
/// change is lost meanwhile), then drain the bus without waiting for a slow channel
/// and route events in order on one blocking worker. A halt must not overtake the claim
/// it reports (HS2-QXG1JQ). Returns once seeding finished.
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
    let (queued, mut pending) = tokio::sync::mpsc::unbounded_channel::<ChangeEvent>();
    tokio::task::spawn_blocking(move || {
        while let Some(event) = pending.blocking_recv() {
            route_change_event(&state, &event);
        }
    });
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if queued.send(event).is_err() {
                        break;
                    }
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
            TicketFlags {
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
            vec![permission_message(state, event)]
        }
        "notification" if wants(&[EventKind::Notification]) => {
            hub_message(state, event).into_iter().collect()
        }
        "activity" | "turn_event" if wants(&[EventKind::ProjectStopped]) => {
            apply_windows(state, &config);
            let next = state
                .notification_delivery
                .activity
                .lock_or_recover()
                .progress(&event.store, now());
            if let Some(at) = next {
                schedule_activity_check(state, &event.store, at);
            }
            Vec::new()
        }
        "terminal_halted" if event.message.is_some() && wants(&[EventKind::ProjectStopped]) => {
            halt_messages(state, event)
        }
        kind if TICKET_KINDS.contains(&kind) => {
            let mut messages = Vec::new();
            if wants(&[EventKind::ProjectStopped]) {
                apply_windows(state, &config);
                messages.extend(observe_activity(state, event));
            }
            if wants(&[
                EventKind::TicketStatusChanged,
                EventKind::TicketFeedbackNeeded,
                EventKind::TicketBlocked,
            ]) {
                messages.extend(ticket_messages(state, &config, event));
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
    let base = hotsheet_notify::links::link_base(state.machine_home(), config);
    let mut records = Vec::new();
    for mut message in messages {
        link(&mut message, base.as_deref(), &|store| {
            state
                .host
                .get(store)
                .map(|entry| entry.store.root().display().to_string())
        });
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

/// Point the message at its project and ticket in the client (HS2-04J8WS). The client
/// resolves a checkout id or a project or store path, so a store-scoped event without a
/// linked checkout links by its store root (`store_root`). Without a client origin, no link.
fn link(message: &mut Message, base: Option<&str>, store_root: &dyn Fn(&str) -> Option<String>) {
    let Some(base) = base.filter(|_| message.url.is_none()) else {
        return;
    };
    let project = message
        .checkouts
        .first()
        .cloned()
        .or_else(|| message.store.as_deref().and_then(store_root))
        .or_else(|| message.project.clone());
    if let Some(project) = project {
        let ticket = message.ticket.as_ref().map(|t| t.slug.as_str());
        message.url = Some(hotsheet_notify::links::deep_link(base, &project, ticket));
    }
}

fn project_name(project: &str) -> String {
    std::path::Path::new(project)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.trim_end_matches(".hs2").to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| project.to_string())
}

/// The checkouts that link `root` (a store or project path).
fn linked(state: &AppState, root: &std::path::Path) -> Vec<Checkout> {
    state
        .checkout_registry
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| {
            crate::watcher::same_path(std::path::Path::new(&c.root), root)
                || c.stores
                    .iter()
                    .any(|s| crate::watcher::same_path(std::path::Path::new(s), root))
        })
        .collect()
}

/// The checkouts that link `root`: their ids, for rule `projects` filters, and the first
/// alias, as the project's display name (HS2-4JG95G).
fn linked_checkouts(state: &AppState, root: &std::path::Path) -> (Vec<String>, Option<String>) {
    let linked = linked(state, root);
    let alias = linked
        .iter()
        .map(|c| c.alias.trim())
        .find(|a| !a.is_empty())
        .map(str::to_owned);
    (linked.into_iter().map(|c| c.id).collect(), alias)
}

/// Checkout ids and display name for a hosted store.
fn store_identity(state: &AppState, store: &str) -> (Vec<String>, String) {
    match state.host.get(store) {
        Some(entry) => {
            let root = entry.store.root().to_path_buf();
            let (checkouts, alias) = linked_checkouts(state, &root);
            let name = alias.unwrap_or_else(|| project_name(&root.display().to_string()));
            (checkouts, name)
        }
        None => (Vec::new(), project_name(store)),
    }
}

fn permission_message(state: &AppState, event: &ChangeEvent) -> Message {
    let (checkouts, alias) = linked_checkouts(state, std::path::Path::new(&event.store));
    let name = alias.unwrap_or_else(|| project_name(&event.store));
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
    message.checkouts = checkouts;
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

fn flags_of(ticket: &Ticket) -> TicketFlags {
    TicketFlags {
        status: wire(ticket.status),
        feedback_needed: ticket.feedback_needed(),
        blocked: ticket
            .blocked_reason
            .as_deref()
            .is_some_and(|r| !r.trim().is_empty())
            || !ticket.blocked_by.is_empty(),
    }
}

/// A ticket as the shared transition logic reads it (HS2-8VAXXA).
pub fn ticket_state(ticket: &Ticket) -> TicketState {
    TicketState {
        id: ticket.id.to_string(),
        slug: ticket.slug.clone(),
        title: ticket.title.clone(),
        category: ticket.category.clone(),
        priority: wire(ticket.priority),
        tags: ticket.tags.clone(),
        blocked_reason: ticket.blocked_reason.clone(),
        flags: flags_of(ticket),
        views: Vec::new(),
    }
}

fn ticket_messages(
    state: &AppState,
    config: &NotificationConfig,
    event: &ChangeEvent,
) -> Vec<Message> {
    let Ok(id) = Ulid::from_string(&event.id) else {
        return Vec::new();
    };
    let Some(entry) = state.host.get(&event.store) else {
        return Vec::new();
    };
    let Ok(ticket) = entry.store.read_ticket(&id) else {
        return Vec::new();
    };
    let mut current = ticket_state(&ticket);
    // HS2-G8NXNC: which saved views a rule names that this ticket is in.
    let checkouts = linked(state, entry.store.root());
    let at = now();
    current.views =
        hotsheet_notify::views::memberships(config, &checkouts, |q| q.matches(&ticket, at));
    let previous = {
        let mut snapshots = state.notification_delivery.snapshots.lock_or_recover();
        if !snapshots.seeded.contains(&event.store) {
            // A store hosted after startup: its first change only records the baseline.
            drop(snapshots);
            seed_store(state, &event.store);
            snapshots = state.notification_delivery.snapshots.lock_or_recover();
        }
        snapshots.tickets.insert(
            (event.store.clone(), event.id.clone()),
            current.flags.clone(),
        )
    };
    let (checkouts, project_name) = store_identity(state, &event.store);
    let origin = Origin {
        store: event.store.clone(),
        checkouts,
        project_name,
    };
    transition_messages(
        previous.as_ref(),
        &current,
        event.kind == "created",
        &origin,
    )
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
                progressed: false,
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
    let fingerprint = progress_fingerprint(&ticket);
    let previous = state
        .notification_delivery
        .fingerprints
        .lock_or_recover()
        .insert((event.store.clone(), event.id.clone()), fingerprint);
    let observation = ClaimObservation {
        ticket: event.id.clone(),
        slug: ticket.slug.clone(),
        claim,
        completed: matches!(
            ticket.status,
            hotsheet_model::Status::Completed | hotsheet_model::Status::Verified
        ),
        // The first sighting has no baseline; a write that changes the fingerprint is
        // progress, a claim renewal alone is not.
        progressed: previous.is_some_and(|p| p != fingerprint),
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
    if let Ok(config) = NotificationConfig::load(state.machine_home()) {
        apply_windows(state, &config);
    }
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
    let (checkouts, name) = store_identity(state, store);
    let (title, severity) = match report.reason {
        StopReason::Finished => (
            format!("Hot Sheet project {name} has stopped working on tickets"),
            Severity::Warning,
        ),
        StopReason::Stalled => (
            format!("Hot Sheet project {name} has stalled"),
            Severity::Error,
        ),
        StopReason::Idle => (
            format!("Hot Sheet project {name} may be stuck"),
            Severity::Warning,
        ),
        StopReason::Halted => (
            format!("Hot Sheet project {name}: an AI session halted"),
            Severity::Error,
        ),
    };
    let held = report
        .held
        .iter()
        .map(|(slug, worker)| format!("**{}** (worker {})", escape(slug), escape(worker)))
        .collect::<Vec<_>>()
        .join(", ");
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
        StopReason::Idle => {
            let minutes = report
                .last_progress
                .map(|at| (OffsetDateTime::now_utc() - at).whole_minutes().max(1))
                .unwrap_or(0);
            format!("No AI progress for about {minutes} minutes while {held} is claimed.")
        }
        StopReason::Halted => format!(
            "The session working {held} halted: {}",
            escape(report.detail.as_deref().unwrap_or("no reason given"))
        ),
    };
    if matches!(report.reason, StopReason::Idle | StopReason::Halted) {
        // The session is still running; a completion summary would be premature.
    } else if report.completed.is_empty() {
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
    message.checkouts = checkouts;
    message.store = Some(store.to_string());
    message.dedupe_key = Some(match report.reason {
        StopReason::Idle => format!(
            "project-idle:{store}:{}",
            report
                .last_progress
                .unwrap_or(report.active_since)
                .unix_timestamp()
        ),
        _ => format!(
            "project-stopped:{store}:{}",
            report.active_since.unix_timestamp()
        ),
    });
    message
}

/// Apply the configured `project.stopped` windows, unless a test fixed the grace.
fn apply_windows(state: &AppState, config: &NotificationConfig) {
    let timing = config.project_activity.clone().unwrap_or_default();
    let grace = state
        .notification_delivery
        .grace_override
        .unwrap_or_else(|| {
            timing
                .grace_seconds
                .map_or(crate::project_activity::IDLE_GRACE, |s| {
                    time::Duration::seconds(s as i64)
                })
        });
    let idle = match timing.idle_minutes {
        Some(0) => None,
        Some(minutes) => Some(time::Duration::minutes(minutes as i64)),
        None => Some(crate::project_activity::IDLE_WINDOW),
    };
    state
        .notification_delivery
        .activity
        .lock_or_recover()
        .set_windows(grace, idle);
}

/// What counts as progress on a ticket: notes, status, phase, and title. A claim renewal
/// changes none of them.
fn progress_fingerprint(ticket: &Ticket) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ticket.notes.len().hash(&mut hasher);
    wire(ticket.status).hash(&mut hasher);
    ticket.started_phase.map(wire).hash(&mut hasher);
    ticket.title.hash(&mut hasher);
    ticket.details.len().hash(&mut hasher);
    hasher.finish()
}

/// A halted terminal: report every store where its worker holds a live claim.
fn halt_messages(state: &AppState, event: &ChangeEvent) -> Vec<Message> {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return Vec::new();
    };
    let Some(worker) =
        runtime.block_on(crate::routes::terminals::terminal_worker(state, &event.id))
    else {
        return Vec::new();
    };
    let halt = event.message.clone().unwrap_or_default();
    let reports = state
        .notification_delivery
        .activity
        .lock_or_recover()
        .halted(&worker, &halt, now());
    reports
        .into_iter()
        .map(|(store, report)| {
            let mut message = stop_message(state, &store, &report);
            message.dedupe_key = Some(format!("project-halt:{store}:{}:{halt}", event.id));
            message
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_prefer_a_checkout_and_keep_an_existing_url() {
        let mut message = Message::new(EventKind::TicketBlocked, "t");
        message.store = Some("store-1".into());
        message.ticket = Some(TicketFacts {
            slug: "HS2-9".into(),
            ..TicketFacts::default()
        });
        let roots = |store: &str| (store == "store-1").then(|| "/work/demo.hs2".to_string());
        link(&mut message, None, &roots);
        assert_eq!(message.url, None, "no client origin, no link");
        link(&mut message, Some("http://hs.test"), &roots);
        assert_eq!(
            message.url.as_deref(),
            Some("http://hs.test/?project=%2Fwork%2Fdemo.hs2&ticket=HS2-9"),
            "a store links by its root, which the client resolves"
        );
        message.url = None;
        message.store = Some("unhosted".into());
        message.project = Some("/work/project".into());
        link(&mut message, Some("http://hs.test"), &roots);
        assert_eq!(
            message.url.as_deref(),
            Some("http://hs.test/?project=%2Fwork%2Fproject&ticket=HS2-9")
        );
        message.url = None;
        message.checkouts = vec!["rocket-ab12".into()];
        link(&mut message, Some("http://hs.test"), &roots);
        assert_eq!(
            message.url.as_deref(),
            Some("http://hs.test/?project=rocket-ab12&ticket=HS2-9")
        );
        message.url = Some("https://elsewhere.test".into());
        link(&mut message, Some("http://hs.test"), &roots);
        assert_eq!(message.url.as_deref(), Some("https://elsewhere.test"));
    }

    #[test]
    fn labels_and_project_names_read_naturally() {
        assert_eq!(project_name("/work/hotsheet2.hs2"), "hotsheet2");
        assert_eq!(project_name("plain"), "plain");
        assert_eq!(escape("a*b_[c]"), "a\\*b\\_\\[c\\]");
    }
}
