//! Rule matching, cooldown suppression, bounded retry, and the delivery log
//! (`docs/32-notifier-plugins.md` §32.7). Delivery is synchronous; the server calls it on
//! a blocking worker so retries never hold a request.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hotsheet_sync::LockExt;
use serde::{Deserialize, Serialize};

use crate::config::NotificationConfig;
use crate::delivery_log::{self, Journal};
use crate::message::{EventKind, Message};
use crate::registry::Registry;
use crate::render::render;
use crate::settings::{self, SecretLookup};

pub const LOG_FILE: &str = "notification-deliveries.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Delivered,
    Failed,
    /// A repeat inside the rule's cooldown.
    Suppressed,
}

/// One channel's delivery of one message. Identifiers and shapes only: the log never
/// holds a title, body, recipient, or attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryRecord {
    pub id: String,
    pub at: String,
    pub event: EventKind,
    pub channel: String,
    pub notifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    pub outcome: Outcome,
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub omitted_attachments: usize,
    /// A short hash of the message's dedupe key, so cooldowns survive process restarts
    /// without the log holding the key text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe: Option<String>,
    /// Suppressed by the channel's quiet hours or hourly limit (not a cooldown repeat), so a
    /// later [`Dispatcher::flush_held`] summary reports it (HS2-CFMSP2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
}

/// Slugs listed per event kind in a held summary before "and N more".
const SUMMARY_SLUGS: usize = 10;

/// Retry schedule for retryable failures: one attempt plus one per backoff entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    pub backoff: Vec<Duration>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            backoff: vec![Duration::from_secs(2), Duration::from_secs(8)],
        }
    }
}

pub type Sleeper = Arc<dyn Fn(Duration) + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> time::OffsetDateTime + Send + Sync>;

/// `(channel, dedupe key)` → last delivery, shared by every dispatcher a long-lived host
/// builds so rule cooldowns survive across dispatches.
pub type Cooldowns = Arc<Mutex<HashMap<(String, String), Instant>>>;

pub struct Dispatcher {
    registry: Arc<Registry>,
    secrets: Arc<dyn SecretLookup + Send + Sync>,
    log_path: Option<PathBuf>,
    retry: RetryPolicy,
    sleeper: Sleeper,
    recent: Cooldowns,
    clock: Clock,
    /// Delivery times per channel, for rate limits when there is no delivery log.
    sent: Mutex<HashMap<String, Vec<time::OffsetDateTime>>>,
}

impl Dispatcher {
    pub fn new(
        registry: Arc<Registry>,
        secrets: Arc<dyn SecretLookup + Send + Sync>,
        home: Option<&Path>,
    ) -> Dispatcher {
        Dispatcher {
            registry,
            secrets,
            log_path: home.map(|h| h.join(LOG_FILE)),
            retry: RetryPolicy::default(),
            sleeper: Arc::new(std::thread::sleep),
            recent: Cooldowns::default(),
            clock: Arc::new(time::OffsetDateTime::now_utc),
            sent: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_retry(mut self, retry: RetryPolicy, sleeper: Sleeper) -> Dispatcher {
        self.retry = retry;
        self.sleeper = sleeper;
        self
    }

    pub fn with_clock(mut self, clock: Clock) -> Dispatcher {
        self.clock = clock;
        self
    }

    pub fn with_cooldowns(mut self, cooldowns: Cooldowns) -> Dispatcher {
        self.recent = cooldowns;
        self
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Deliver `message` to every enabled channel an enabled matching rule names. Each
    /// channel receives one copy even when several rules match. `Test` messages go
    /// nowhere here; use [`Dispatcher::deliver_to`].
    pub fn dispatch(&self, config: &NotificationConfig, message: &Message) -> Vec<DeliveryRecord> {
        if matches!(message.event, EventKind::Test | EventKind::HeldSummary) {
            return Vec::new();
        }
        // A channel whose hold has ended reports what it held before this message.
        let mut records = self.flush_held(config);
        let mut targets: Vec<(&str, &str, u64)> = Vec::new();
        for rule in config.rules.iter().filter(|r| r.matches(message)) {
            for channel in &rule.channels {
                if !targets.iter().any(|(c, _, _)| c == channel) {
                    targets.push((channel, &rule.id, rule.cooldown_seconds));
                }
            }
        }
        records.extend(targets.into_iter().filter_map(|(channel, rule, cooldown)| {
            let channel = config.channel(channel).filter(|c| c.enabled)?;
            let held = self.held(channel, message.severity);
            let reason = held.clone().or_else(|| {
                self.suppressed(&channel.id, message, cooldown)
                    .then(|| "a repeat inside the rule's cooldown".to_string())
            });
            if let Some(reason) = reason {
                let mut record = self.record(
                    message,
                    &channel.id,
                    &channel.notifier,
                    Some(rule),
                    Outcome::Suppressed,
                );
                record.error = Some(reason);
                record.held = held.is_some();
                return Some(self.finish(record));
            }
            Some(self.deliver(config, &channel.id, Some(rule), message))
        }));
        records
    }

    /// Send each enabled channel one summary of the deliveries its quiet hours or hourly
    /// limit held. Process-shared locks and durable exact-ID reservations prevent a server
    /// timer and serverless writer from sending the same interval (HS2-MFD44D). An
    /// interrupted send is recovered as unknown/failed, never silently resent.
    pub fn flush_held(&self, config: &NotificationConfig) -> Vec<DeliveryRecord> {
        match self.try_flush_held(config) {
            Ok(records) => records,
            Err(error) => {
                eprintln!("hotsheet-notify: could not flush held summaries: {error}");
                Vec::new()
            }
        }
    }

    fn try_flush_held(&self, config: &NotificationConfig) -> std::io::Result<Vec<DeliveryRecord>> {
        let Some(path) = &self.log_path else {
            return Ok(Vec::new());
        };
        // Lock order is flush -> journal. No journal lock crosses a notifier call.
        let Some(_flushing) = delivery_log::lock_flush(path)? else {
            return Ok(Vec::new());
        };
        let mut sent = Journal::open(path)?.recover_interrupted()?;
        for channel in config.channels.iter().filter(|channel| channel.enabled) {
            if self.held(channel, crate::message::Severity::Info).is_some() {
                continue;
            }
            let (message, record) = {
                let mut journal = Journal::open(path)?;
                let held = journal.held(&channel.id);
                if held.is_empty() {
                    continue;
                }
                let message = held_summary(&held.iter().collect::<Vec<_>>());
                let record = self.record(
                    &message,
                    &channel.id,
                    &channel.notifier,
                    None,
                    Outcome::Failed,
                );
                // Failure to persist/sync the reservation stops before any transport work.
                journal.reserve(&record, &held)?;
                (message, record)
            };
            let record = self.attempt_delivery(config, &channel.id, &message, record);
            Journal::open(path)?.append_record(&record)?;
            sent.push(record);
        }
        Ok(sent)
    }

    /// Deliver to one named channel regardless of rules or its enabled flag (a test or an
    /// explicit send).
    pub fn deliver_to(
        &self,
        config: &NotificationConfig,
        channel: &str,
        message: &Message,
    ) -> Result<DeliveryRecord, String> {
        if config.channel(channel).is_none() {
            return Err(format!("unknown channel '{channel}'"));
        }
        Ok(self.deliver(config, channel, None, message))
    }

    /// Why a rule-routed delivery of `severity` to `channel` is held right now: quiet hours
    /// or the hourly limit (HS2-4JG95G). Tests and explicit sends bypass both.
    fn held(
        &self,
        channel: &crate::config::ChannelConfig,
        severity: crate::message::Severity,
    ) -> Option<String> {
        let now = (self.clock)();
        if let Some(quiet) = &channel.quiet_hours {
            let urgent = quiet.allow_errors && severity >= crate::message::Severity::Error;
            if quiet.contains(now) && !urgent {
                return Some(format!(
                    "quiet hours {}-{} ({})",
                    quiet.start, quiet.end, quiet.utc_offset
                ));
            }
        }
        let limit = channel.max_per_hour?;
        let since = now - time::Duration::hours(1);
        let count = match &self.log_path {
            Some(path) => {
                delivery_log::read_records(path)
                    .unwrap_or_default()
                    .into_iter()
                    // A held summary reports on the limit; counting it would hold the next
                    // message and start another summary.
                    .filter(|r| {
                        r.channel == channel.id
                            && r.outcome == Outcome::Delivered
                            && r.event != EventKind::HeldSummary
                    })
                    .filter_map(|r| {
                        time::OffsetDateTime::parse(
                            &r.at,
                            &time::format_description::well_known::Rfc3339,
                        )
                        .ok()
                    })
                    .filter(|at| *at > since)
                    .count()
            }
            None => self
                .sent
                .lock_or_recover()
                .get(&channel.id)
                .map_or(0, |times| times.iter().filter(|at| **at > since).count()),
        };
        (count >= limit as usize).then(|| format!("rate limit of {limit} per hour reached"))
    }

    /// Whether `channel` already took this dedupe key inside the cooldown: from this
    /// process's memory, else from the delivery log (a restarted server or another CLI run).
    fn suppressed(&self, channel: &str, message: &Message, cooldown: u64) -> bool {
        let Some(key) = message.dedupe_key.as_ref().filter(|_| cooldown > 0) else {
            return false;
        };
        let mut recent = self.recent.lock_or_recover();
        let now = Instant::now();
        let window = Duration::from_secs(cooldown);
        recent.retain(|_, at| now.duration_since(*at) < Duration::from_secs(24 * 3600));
        let slot = (channel.to_string(), key.clone());
        if recent
            .get(&slot)
            .is_some_and(|at| now.duration_since(*at) < window)
        {
            return true;
        }
        if self.logged_within(channel, &dedupe_hash(key), window) {
            return true;
        }
        recent.insert(slot, now);
        false
    }

    fn logged_within(&self, channel: &str, hash: &str, window: Duration) -> bool {
        let Some(path) = &self.log_path else {
            return false;
        };
        let Ok(records) = delivery_log::read_records(path) else {
            return false;
        };
        let now = (self.clock)();
        records
            .into_iter()
            .rev()
            .filter(|r| {
                r.channel == channel
                    && r.outcome == Outcome::Delivered
                    && r.dedupe.as_deref() == Some(hash)
            })
            .filter_map(|r| {
                time::OffsetDateTime::parse(&r.at, &time::format_description::well_known::Rfc3339)
                    .ok()
            })
            .any(|at| (now - at).whole_seconds() < window.as_secs() as i64)
    }

    fn deliver(
        &self,
        config: &NotificationConfig,
        channel_id: &str,
        rule: Option<&str>,
        message: &Message,
    ) -> DeliveryRecord {
        let channel = config
            .channel(channel_id)
            .expect("caller checked the channel");
        let record = self.record(
            message,
            channel_id,
            &channel.notifier,
            rule,
            Outcome::Failed,
        );
        self.finish(self.attempt_delivery(config, channel_id, message, record))
    }

    fn attempt_delivery(
        &self,
        config: &NotificationConfig,
        channel_id: &str,
        message: &Message,
        mut record: DeliveryRecord,
    ) -> DeliveryRecord {
        let channel = config
            .channel(channel_id)
            .expect("caller checked the channel");
        let Some(notifier) = self.registry.get(&channel.notifier) else {
            record.error = Some(format!("notifier '{}' is not installed", channel.notifier));
            return record;
        };
        if let Some(current) = notifier.content_digest() {
            let refusal = match (current, channel.plugin_digest.as_deref()) {
                (Ok(current), Some(approved)) if current == approved => None,
                (Ok(_), Some(_)) => Some(format!(
                    "notifier plugin '{}' changed since channel '{channel_id}' approved it; review the plugin, then run `hotsheet-cli notify channel trust {channel_id}`",
                    channel.notifier
                )),
                (Ok(_), None) => Some(format!(
                    "channel '{channel_id}' has not approved notifier plugin '{}'; review the plugin, then run `hotsheet-cli notify channel trust {channel_id}`",
                    channel.notifier
                )),
                (Err(error), _) => Some(format!(
                    "cannot verify notifier plugin '{}': {error}",
                    channel.notifier
                )),
            };
            if let Some(refusal) = refusal {
                record.error = Some(refusal);
                return record;
            }
        }
        let descriptor = notifier.descriptor();
        let resolved = match settings::resolve(
            &descriptor.settings,
            &channel.settings,
            self.secrets.as_ref(),
        ) {
            Ok(resolved) => resolved,
            Err(error) => {
                record.error = Some(error);
                return record;
            }
        };
        let rendered = render(message, &descriptor.capabilities);
        record.truncated = rendered.truncated;
        record.omitted_attachments = rendered.omitted_attachments;
        let mut backoff = self.retry.backoff.iter();
        loop {
            record.attempts += 1;
            match notifier.deliver(channel_id, &rendered, &resolved) {
                Ok(provider_id) => {
                    let now = (self.clock)();
                    let mut sent = self.sent.lock_or_recover();
                    let times = sent.entry(channel_id.to_string()).or_default();
                    times.retain(|at| now - *at < time::Duration::hours(1));
                    times.push(now);
                    drop(sent);
                    record.outcome = Outcome::Delivered;
                    record.provider_id = provider_id;
                    record.error = None;
                    return record;
                }
                Err(error) => {
                    record.error = Some(error.message.clone());
                    match backoff.next() {
                        Some(delay) if error.retryable => (self.sleeper)(*delay),
                        _ => return record,
                    }
                }
            }
        }
    }

    fn record(
        &self,
        message: &Message,
        channel: &str,
        notifier: &str,
        rule: Option<&str>,
        outcome: Outcome,
    ) -> DeliveryRecord {
        DeliveryRecord {
            id: ulid::Ulid::new().to_string(),
            at: (self.clock)()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            event: message.event,
            channel: channel.into(),
            notifier: notifier.into(),
            rule: rule.map(str::to_owned),
            outcome,
            attempts: 0,
            error: None,
            provider_id: None,
            project: message.project.clone(),
            ticket: message.ticket.as_ref().map(|t| t.slug.clone()),
            truncated: false,
            omitted_attachments: 0,
            dedupe: message.dedupe_key.as_deref().map(dedupe_hash),
            held: false,
        }
    }

    fn finish(&self, record: DeliveryRecord) -> DeliveryRecord {
        if let Some(path) = &self.log_path {
            if let Err(error) = delivery_log::append_record(path, &record) {
                eprintln!(
                    "hotsheet-notify: could not write {}: {error}",
                    path.display()
                );
            }
        }
        record
    }
}

/// "3 notifications were held": a count and ticket slugs per event kind, then the reasons.
/// The log keeps identifiers only, so the summary names no ticket titles.
fn held_summary(held: &[&DeliveryRecord]) -> Message {
    let count = held.len();
    let mut message = Message::new(
        EventKind::HeldSummary,
        if count == 1 {
            "1 notification was held".to_string()
        } else {
            format!("{count} notifications were held")
        },
    );
    let mut kinds: Vec<EventKind> = Vec::new();
    for record in held {
        if !kinds.contains(&record.event) {
            kinds.push(record.event);
        }
    }
    let mut lines = Vec::new();
    for kind in kinds {
        let of_kind: Vec<_> = held.iter().filter(|r| r.event == kind).collect();
        let mut slugs: Vec<&str> = Vec::new();
        for slug in of_kind.iter().filter_map(|r| r.ticket.as_deref()) {
            if !slugs.contains(&slug) {
                slugs.push(slug);
            }
        }
        let mut line = format!("- {} ×{}", kind.label(), of_kind.len());
        if !slugs.is_empty() {
            let shown = slugs.len().min(SUMMARY_SLUGS);
            line.push_str(&format!(": {}", slugs[..shown].join(", ")));
            if slugs.len() > shown {
                line.push_str(&format!(" and {} more", slugs.len() - shown));
            }
        }
        lines.push(line);
    }
    let mut reasons: Vec<&str> = Vec::new();
    for reason in held.iter().filter_map(|r| r.error.as_deref()) {
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
    }
    message.body = format!(
        "{}\n\nHeld by {}.",
        lines.join("\n"),
        crate::transitions::escape(&reasons.join("; "))
    );
    message
}

/// The first 16 hex digits of the key's SHA-256.
fn dedupe_hash(key: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(key.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The newest `limit` delivery records, newest first. Unparseable lines are skipped.
pub fn recent_deliveries(home: &Path, limit: usize) -> Vec<DeliveryRecord> {
    delivery_log::read_records(&home.join(LOG_FILE))
        .unwrap_or_default()
        .into_iter()
        .rev()
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::ntfy::Ntfy;
    use crate::config::{ChannelConfig, Rule, RuleFilter};
    use crate::http::HttpResponse;
    use crate::notifier::{CommandRunner, LiveRunner};
    use crate::testing::FakeHttp;
    use serde_json::json;
    use std::collections::BTreeMap;

    struct Harness {
        http: Arc<FakeHttp>,
        sleeps: Arc<Mutex<Vec<Duration>>>,
        dispatcher: Dispatcher,
        home: tempfile::TempDir,
    }

    fn harness(replies: Vec<Result<HttpResponse, String>>) -> Harness {
        let http = Arc::new(FakeHttp::sequence(replies));
        let runner: Arc<dyn CommandRunner> = Arc::new(LiveRunner);
        let registry = Arc::new(Registry::with_builtins(
            vec![Arc::new(Ntfy::with_transport(http.clone()))],
            None,
            runner,
        ));
        let mut secrets = BTreeMap::new();
        secrets.insert("ntfy-token".to_string(), "tk".to_string());
        let home = tempfile::tempdir().unwrap();
        let sleeps = Arc::new(Mutex::new(Vec::new()));
        let recorded = sleeps.clone();
        let dispatcher = Dispatcher::new(registry, Arc::new(secrets), Some(home.path()))
            .with_retry(
                RetryPolicy::default(),
                Arc::new(move |d| recorded.lock_or_recover().push(d)),
            );
        Harness {
            http,
            sleeps,
            dispatcher,
            home,
        }
    }

    fn ok() -> Result<HttpResponse, String> {
        Ok(HttpResponse {
            status: 200,
            body: r#"{"id":"n1"}"#.into(),
        })
    }

    fn status(code: u16) -> Result<HttpResponse, String> {
        Ok(HttpResponse {
            status: code,
            body: "e".into(),
        })
    }

    fn channel(id: &str) -> ChannelConfig {
        ChannelConfig {
            id: id.into(),
            notifier: "ntfy".into(),
            label: None,
            enabled: true,
            settings: serde_json::from_value(
                json!({"topic": id, "token": {"secret":"ntfy-token"}}),
            )
            .unwrap(),
            plugin_digest: None,
            quiet_hours: None,
            max_per_hour: None,
        }
    }

    fn config(cooldown: u64) -> NotificationConfig {
        NotificationConfig {
            channels: vec![channel("phone"), channel("desk")],
            rules: vec![
                Rule {
                    id: "blocked".into(),
                    events: vec![EventKind::TicketBlocked],
                    channels: vec!["phone".into(), "desk".into()],
                    enabled: true,
                    filter: RuleFilter::default(),
                    cooldown_seconds: cooldown,
                },
                Rule {
                    id: "also".into(),
                    events: vec![EventKind::TicketBlocked],
                    channels: vec!["phone".into()],
                    enabled: true,
                    filter: RuleFilter::default(),
                    cooldown_seconds: 0,
                },
            ],
            ..NotificationConfig::default()
        }
    }

    fn blocked() -> Message {
        let mut m = Message::new(EventKind::TicketBlocked, "HS2-1 blocked");
        m.dedupe_key = Some("blocked:HS2-1".into());
        m
    }

    #[test]
    fn delivers_once_per_channel_across_matching_rules_and_logs_shapes_only() {
        let h = harness(vec![ok()]);
        let records = h.dispatcher.dispatch(&config(0), &blocked());
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .all(|r| r.outcome == Outcome::Delivered && r.attempts == 1)
        );
        assert_eq!(records[0].rule.as_deref(), Some("blocked"));
        assert_eq!(records[0].provider_id.as_deref(), Some("n1"));
        let urls: Vec<_> = h.http.requests().into_iter().map(|r| r.url).collect();
        assert_eq!(urls, ["https://ntfy.sh/phone", "https://ntfy.sh/desk"]);
        let log = std::fs::read_to_string(h.home.path().join(LOG_FILE)).unwrap();
        assert!(
            !log.contains("HS2-1 blocked"),
            "the log must not hold message text"
        );
        assert_eq!(recent_deliveries(h.home.path(), 10).len(), 2);
        assert_eq!(recent_deliveries(h.home.path(), 1)[0].channel, "desk");
    }

    #[test]
    fn disabled_channels_unmatched_events_and_tests_are_skipped() {
        let h = harness(vec![ok()]);
        let mut c = config(0);
        c.channels[1].enabled = false;
        assert_eq!(h.dispatcher.dispatch(&c, &blocked()).len(), 1);
        assert!(
            h.dispatcher
                .dispatch(&c, &Message::new(EventKind::Digest, "d"))
                .is_empty()
        );
        assert!(
            h.dispatcher
                .dispatch(&c, &Message::new(EventKind::Test, "t"))
                .is_empty()
        );
        // An explicit send ignores rules and the enabled flag.
        let r = h
            .dispatcher
            .deliver_to(&c, "desk", &Message::new(EventKind::Test, "t"))
            .unwrap();
        assert_eq!((r.outcome, r.rule), (Outcome::Delivered, None));
        assert!(h.dispatcher.deliver_to(&c, "nope", &blocked()).is_err());
    }

    #[test]
    fn cooldown_suppresses_repeats_per_channel_and_key() {
        let h = harness(vec![ok()]);
        let c = config(600);
        assert!(
            h.dispatcher
                .dispatch(&c, &blocked())
                .iter()
                .all(|r| r.outcome == Outcome::Delivered)
        );
        let again = h.dispatcher.dispatch(&c, &blocked());
        assert!(
            again
                .iter()
                .all(|r| r.outcome == Outcome::Suppressed && r.attempts == 0)
        );
        let mut other = blocked();
        other.dedupe_key = Some("blocked:HS2-2".into());
        assert!(
            h.dispatcher
                .dispatch(&c, &other)
                .iter()
                .all(|r| r.outcome == Outcome::Delivered)
        );
        let mut keyless = blocked();
        keyless.dedupe_key = None;
        assert!(
            h.dispatcher
                .dispatch(&c, &keyless)
                .iter()
                .all(|r| r.outcome == Outcome::Delivered)
        );
        assert_eq!(h.http.requests().len(), 6);
    }

    #[test]
    fn cooldown_survives_a_new_dispatcher_through_the_log() {
        let h = harness(vec![ok()]);
        let c = config(600);
        h.dispatcher.dispatch(&c, &blocked());
        let log = std::fs::read_to_string(h.home.path().join(LOG_FILE)).unwrap();
        assert!(
            !log.contains("blocked:HS2-1"),
            "the log stores a hash, not the key"
        );
        let fresh = Dispatcher::new(
            Arc::new(Registry::with_builtins(
                vec![Arc::new(Ntfy::with_transport(h.http.clone()))],
                None,
                Arc::new(LiveRunner),
            )),
            Arc::new(BTreeMap::from([(
                "ntfy-token".to_string(),
                "tk".to_string(),
            )])),
            Some(h.home.path()),
        );
        let again = fresh.dispatch(&c, &blocked());
        assert!(again.iter().all(|r| r.outcome == Outcome::Suppressed));
        // A failed delivery never starts a cooldown.
        let failing = harness(vec![status(401)]);
        failing.dispatcher.dispatch(&c, &blocked());
        let retry = Dispatcher::new(
            Arc::new(Registry::with_builtins(
                vec![Arc::new(Ntfy::with_transport(failing.http.clone()))],
                None,
                Arc::new(LiveRunner),
            )),
            Arc::new(BTreeMap::new()),
            Some(failing.home.path()),
        );
        assert!(
            retry
                .dispatch(&c, &blocked())
                .iter()
                .all(|r| r.outcome != Outcome::Suppressed)
        );
    }

    /// A dispatcher over `h`'s transport and log whose clock reads `clock`.
    fn at(h: &Harness, clock: Arc<Mutex<time::OffsetDateTime>>, log: bool) -> Dispatcher {
        let registry = Arc::new(Registry::with_builtins(
            vec![Arc::new(Ntfy::with_transport(h.http.clone()))],
            None,
            Arc::new(LiveRunner),
        ));
        let secrets = BTreeMap::from([("ntfy-token".to_string(), "tk".to_string())]);
        Dispatcher::new(registry, Arc::new(secrets), log.then(|| h.home.path()))
            .with_clock(Arc::new(move || *clock.lock_or_recover()))
    }

    fn utc(h: u8, m: u8) -> time::OffsetDateTime {
        time::OffsetDateTime::UNIX_EPOCH
            .replace_date(time::Date::from_calendar_date(2026, time::Month::October, 29).unwrap())
            .replace_time(time::Time::from_hms(h, m, 0).unwrap())
    }

    fn one_channel(cooldown: u64) -> NotificationConfig {
        let mut c = config(cooldown);
        c.rules.truncate(1);
        c.rules[0].channels = vec!["phone".into()];
        c
    }

    #[test]
    fn hs2_4jg95g_quiet_hours_hold_routine_messages_but_not_errors_or_tests() {
        let h = harness(vec![ok()]);
        let clock = Arc::new(Mutex::new(utc(23, 0)));
        let d = at(&h, clock.clone(), true);
        let mut c = one_channel(0);
        c.channels[0].quiet_hours = Some(crate::config::QuietHours {
            start: "22:00".into(),
            end: "07:00".into(),
            utc_offset: "+00:00".into(),
            allow_errors: true,
        });
        let held = d.dispatch(&c, &blocked()).remove(0);
        assert_eq!(held.outcome, Outcome::Suppressed);
        assert_eq!(
            held.error.as_deref(),
            Some("quiet hours 22:00-07:00 (+00:00)")
        );
        assert!(h.http.requests().is_empty());
        let mut urgent = blocked();
        urgent.severity = crate::message::Severity::Error;
        assert_eq!(
            d.dispatch(&c, &urgent).remove(0).outcome,
            Outcome::Delivered
        );
        c.channels[0].quiet_hours.as_mut().unwrap().allow_errors = false;
        assert_eq!(
            d.dispatch(&c, &urgent).remove(0).outcome,
            Outcome::Suppressed
        );
        // A test or explicit send ignores quiet hours.
        assert_eq!(
            d.deliver_to(&c, "phone", &blocked()).unwrap().outcome,
            Outcome::Delivered
        );
        *clock.lock_or_recover() = utc(7, 0);
        assert_eq!(
            d.dispatch(&c, &blocked()).pop().unwrap().outcome,
            Outcome::Delivered
        );
        assert!(
            recent_deliveries(h.home.path(), 1)[0]
                .at
                .starts_with("2026-10-29T07:00:00")
        );
    }

    fn quiet(c: &mut NotificationConfig) {
        c.channels[0].quiet_hours = Some(crate::config::QuietHours {
            start: "22:00".into(),
            end: "07:00".into(),
            utc_offset: "+00:00".into(),
            allow_errors: true,
        });
    }

    fn ticket(event: EventKind, slug: &str) -> Message {
        let mut m = Message::new(event, format!("{slug} changed"));
        m.ticket = Some(crate::message::TicketFacts {
            slug: slug.into(),
            ..Default::default()
        });
        m
    }

    fn summaries(h: &Harness) -> Vec<DeliveryRecord> {
        recent_deliveries(h.home.path(), 100)
            .into_iter()
            .filter(|r| r.event == EventKind::HeldSummary)
            .collect()
    }

    #[test]
    fn hs2_cfmsp2_quiet_hours_end_with_one_summary_of_what_was_held() {
        let h = harness(vec![ok()]);
        let clock = Arc::new(Mutex::new(utc(23, 0)));
        let d = at(&h, clock.clone(), true);
        let mut c = one_channel(600);
        c.rules[0].events = vec![EventKind::TicketBlocked, EventKind::TicketStatusChanged];
        quiet(&mut c);
        for (event, slug) in [
            (EventKind::TicketBlocked, "HS2-1"),
            (EventKind::TicketBlocked, "HS2-2"),
            (EventKind::TicketBlocked, "HS2-1"),
            (EventKind::TicketStatusChanged, "HS2-3"),
        ] {
            let held = d.dispatch(&c, &ticket(event, slug)).pop().unwrap();
            assert!(held.held && held.outcome == Outcome::Suppressed);
        }
        // Still quiet: nothing is summarized yet.
        assert!(d.flush_held(&c).is_empty());
        assert!(h.http.requests().is_empty());

        // After the window a fresh dispatcher (a restart, or another CLI run) reads the log.
        *clock.lock_or_recover() = utc(7, 5);
        let fresh = at(&h, clock.clone(), true);
        let sent = fresh.flush_held(&c);
        assert_eq!(sent.len(), 1);
        assert_eq!(
            (sent[0].event, sent[0].outcome, sent[0].rule.as_deref()),
            (EventKind::HeldSummary, Outcome::Delivered, None)
        );
        let request = h.http.requests().remove(0);
        let header = |name: &str| {
            request
                .headers
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            header("Title").as_deref(),
            Some("4 notifications were held")
        );
        let body = String::from_utf8(request.body).unwrap();
        assert!(body.contains("Blocked ×3: HS2-1, HS2-2"), "{body}");
        assert!(body.contains("Status Changed ×1: HS2-3"), "{body}");
        assert!(
            body.contains("Held by quiet hours 22:00-07:00 (+00:00)."),
            "{body}"
        );

        // Once summarized, nothing repeats; a later message is delivered on its own.
        assert!(fresh.flush_held(&c).is_empty());
        let next = fresh.dispatch(&c, &ticket(EventKind::TicketBlocked, "HS2-9"));
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].outcome, Outcome::Delivered);
        assert_eq!(summaries(&h).len(), 1);
    }

    #[test]
    fn hs2_cfmsp2_rate_limit_recovery_summarizes_before_the_next_message() {
        let h = harness(vec![ok()]);
        let clock = Arc::new(Mutex::new(utc(9, 0)));
        let d = at(&h, clock.clone(), true);
        let mut c = one_channel(0);
        c.channels[0].max_per_hour = Some(1);
        let m = |slug: &str| ticket(EventKind::TicketBlocked, slug);
        assert_eq!(d.dispatch(&c, &m("HS2-1"))[0].outcome, Outcome::Delivered);
        let held = d.dispatch(&c, &m("HS2-2")).remove(0);
        assert!(held.held, "{held:?}");
        assert_eq!(
            held.error.as_deref(),
            Some("rate limit of 1 per hour reached")
        );
        // Over the limit, a dispatch flushes nothing and holds again.
        assert!(d.dispatch(&c, &m("HS2-3"))[0].held);

        *clock.lock_or_recover() = utc(10, 1);
        let records = d.dispatch(&c, &m("HS2-4"));
        assert_eq!(
            records
                .iter()
                .map(|r| (r.event, r.outcome))
                .collect::<Vec<_>>(),
            [
                (EventKind::HeldSummary, Outcome::Delivered),
                (EventKind::TicketBlocked, Outcome::Delivered),
            ],
            "the summary does not use up the hour's budget"
        );
        let titles: Vec<_> = h
            .http
            .requests()
            .into_iter()
            .filter_map(|r| r.headers.into_iter().find(|(n, _)| n == "Title"))
            .map(|(_, v)| v)
            .collect();
        assert_eq!(
            titles,
            [
                "HS2-1 changed",
                "2 notifications were held",
                "HS2-4 changed"
            ]
        );
    }

    #[test]
    fn hs2_cfmsp2_cooldown_repeats_disabled_channels_and_failures_are_not_resummarized() {
        let h = harness(vec![status(400)]);
        let clock = Arc::new(Mutex::new(utc(12, 0)));
        let d = at(&h, clock.clone(), true);
        let mut c = one_channel(600);
        // A cooldown repeat is suppressed but not held, so it is never summarized.
        d.dispatch(&c, &blocked());
        let repeat = d.dispatch(&c, &blocked()).remove(0);
        assert!(repeat.outcome == Outcome::Suppressed && !repeat.held);
        assert!(d.flush_held(&c).is_empty());

        quiet(&mut c);
        *clock.lock_or_recover() = utc(23, 0);
        d.dispatch(&c, &ticket(EventKind::TicketBlocked, "HS2-5"));
        *clock.lock_or_recover() = utc(8, 0);
        c.channels[0].enabled = false;
        assert!(d.flush_held(&c).is_empty(), "a disabled channel waits");
        c.channels[0].enabled = true;
        let failed = d.flush_held(&c);
        assert_eq!(
            (failed[0].event, failed[0].outcome),
            (EventKind::HeldSummary, Outcome::Failed)
        );
        assert!(
            d.flush_held(&c).is_empty(),
            "a failed summary is not resent"
        );
        // No log, no memory of what was held.
        let unlogged = at(&h, clock, false);
        assert!(unlogged.flush_held(&c).is_empty());
        // Rules cannot choose the summary event.
        c.rules[0].events = vec![EventKind::HeldSummary];
        assert!(
            c.validate(&Registry::builtin_only())
                .unwrap_err()
                .to_string()
                .contains("held-notification summaries")
        );
    }

    #[test]
    fn hs2_4jg95g_hourly_rate_limit_counts_the_log_and_memory() {
        for log in [true, false] {
            let h = harness(vec![ok()]);
            let clock = Arc::new(Mutex::new(utc(9, 0)));
            let d = at(&h, clock.clone(), log);
            let mut c = one_channel(0);
            c.channels[0].max_per_hour = Some(2);
            let mut m = blocked();
            m.dedupe_key = None;
            // The message's own record is last; a held summary may precede it.
            let outcomes = |d: &Dispatcher| d.dispatch(&c, &m).pop().unwrap();
            assert_eq!(outcomes(&d).outcome, Outcome::Delivered);
            *clock.lock_or_recover() = utc(9, 30);
            assert_eq!(outcomes(&d).outcome, Outcome::Delivered);
            let limited = outcomes(&d);
            assert_eq!(limited.outcome, Outcome::Suppressed);
            assert_eq!(
                limited.error.as_deref(),
                Some("rate limit of 2 per hour reached")
            );
            if log {
                // A restarted process reads the same limit from the delivery log.
                assert_eq!(
                    outcomes(&at(&h, clock.clone(), true)).outcome,
                    Outcome::Suppressed
                );
            }
            // The 09:00 delivery leaves the window after an hour.
            *clock.lock_or_recover() = utc(10, 1);
            assert_eq!(outcomes(&d).outcome, Outcome::Delivered);
            assert_eq!(outcomes(&d).outcome, Outcome::Suppressed);
            // With a log, the 10:01 dispatch first summarizes the held one (HS2-CFMSP2).
            assert_eq!(h.http.requests().len(), if log { 4 } else { 3 });
        }
    }

    #[test]
    fn retries_retryable_failures_with_backoff_then_gives_up() {
        let h = harness(vec![status(503), status(503), ok()]);
        let mut c = config(0);
        c.rules.truncate(1);
        c.rules[0].channels = vec!["phone".into()];
        let r = h.dispatcher.dispatch(&c, &blocked()).remove(0);
        assert_eq!(
            (r.outcome, r.attempts, r.error),
            (Outcome::Delivered, 3, None)
        );
        assert_eq!(
            *h.sleeps.lock_or_recover(),
            [Duration::from_secs(2), Duration::from_secs(8)]
        );

        let h = harness(vec![status(503)]);
        let r = h.dispatcher.dispatch(&c, &blocked()).remove(0);
        assert_eq!((r.outcome, r.attempts), (Outcome::Failed, 3));
        assert!(r.error.unwrap().contains("503"));

        let h = harness(vec![status(401)]);
        let r = h.dispatcher.dispatch(&c, &blocked()).remove(0);
        assert_eq!((r.outcome, r.attempts), (Outcome::Failed, 1));
        assert!(h.sleeps.lock_or_recover().is_empty());
    }

    #[test]
    fn missing_secret_or_notifier_fails_without_sending() {
        let h = harness(vec![ok()]);
        let mut c = config(0);
        c.channels[0]
            .settings
            .insert("token".into(), json!({"secret":"absent"}));
        c.channels[1].notifier = "uninstalled".into();
        let records = h.dispatcher.dispatch(&c, &blocked());
        assert!(
            records
                .iter()
                .all(|r| r.outcome == Outcome::Failed && r.attempts == 0)
        );
        assert!(
            records[0]
                .error
                .as_ref()
                .unwrap()
                .contains("credential for 'token'")
        );
        assert!(records[1].error.as_ref().unwrap().contains("not installed"));
        assert!(h.http.requests().is_empty());
    }

    #[test]
    fn log_compacts_past_its_cap() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(LOG_FILE);
        let h = harness(vec![ok()]);
        let mut record = h.dispatcher.dispatch(&config(0), &blocked()).remove(0);
        for i in 0..=delivery_log::COMPACT_AT {
            record.id = i.to_string();
            delivery_log::append_record(&path, &record).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap().lines().count(),
            delivery_log::KEEP
        );
        assert_eq!(
            recent_deliveries(home.path(), 1)[0].id,
            delivery_log::COMPACT_AT.to_string()
        );
    }
}
