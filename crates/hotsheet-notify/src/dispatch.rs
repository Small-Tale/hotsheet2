//! Rule matching, cooldown suppression, bounded retry, and the delivery log
//! (`docs/32-notifier-plugins.md` §32.7). Delivery is synchronous; the server calls it on
//! a blocking worker so retries never hold a request.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hotsheet_sync::LockExt;
use serde::{Deserialize, Serialize};

use crate::config::NotificationConfig;
use crate::message::{EventKind, Message};
use crate::registry::Registry;
use crate::render::render;
use crate::settings::{self, SecretLookup};

pub const LOG_FILE: &str = "notification-deliveries.jsonl";
/// The log is compacted to [`LOG_KEEP`] records once it passes this many lines.
const LOG_COMPACT_AT: usize = 1000;
const LOG_KEEP: usize = 500;

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
}

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
    log_lock: Mutex<()>,
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
            log_lock: Mutex::new(()),
        }
    }

    pub fn with_retry(mut self, retry: RetryPolicy, sleeper: Sleeper) -> Dispatcher {
        self.retry = retry;
        self.sleeper = sleeper;
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
        if message.event == EventKind::Test {
            return Vec::new();
        }
        let mut targets: Vec<(&str, &str, u64)> = Vec::new();
        for rule in config.rules.iter().filter(|r| r.matches(message)) {
            for channel in &rule.channels {
                if !targets.iter().any(|(c, _, _)| c == channel) {
                    targets.push((channel, &rule.id, rule.cooldown_seconds));
                }
            }
        }
        targets
            .into_iter()
            .filter_map(|(channel, rule, cooldown)| {
                let channel = config.channel(channel).filter(|c| c.enabled)?;
                if self.suppressed(&channel.id, message, cooldown) {
                    return Some(self.finish(self.record(
                        message,
                        &channel.id,
                        &channel.notifier,
                        Some(rule),
                        Outcome::Suppressed,
                    )));
                }
                Some(self.deliver(config, &channel.id, Some(rule), message))
            })
            .collect()
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
        let _guard = self.log_lock.lock_or_recover();
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        let now = time::OffsetDateTime::now_utc();
        text.lines()
            .rev()
            .filter_map(|line| serde_json::from_str::<DeliveryRecord>(line).ok())
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
        let mut record = self.record(
            message,
            channel_id,
            &channel.notifier,
            rule,
            Outcome::Failed,
        );
        let Some(notifier) = self.registry.get(&channel.notifier) else {
            record.error = Some(format!("notifier '{}' is not installed", channel.notifier));
            return self.finish(record);
        };
        let descriptor = notifier.descriptor();
        let resolved = match settings::resolve(
            &descriptor.settings,
            &channel.settings,
            self.secrets.as_ref(),
        ) {
            Ok(resolved) => resolved,
            Err(error) => {
                record.error = Some(error);
                return self.finish(record);
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
                    record.outcome = Outcome::Delivered;
                    record.provider_id = provider_id;
                    record.error = None;
                    return self.finish(record);
                }
                Err(error) => {
                    record.error = Some(error.message.clone());
                    match backoff.next() {
                        Some(delay) if error.retryable => (self.sleeper)(*delay),
                        _ => return self.finish(record),
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
            at: now(),
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
        }
    }

    fn finish(&self, record: DeliveryRecord) -> DeliveryRecord {
        if let Some(path) = &self.log_path {
            let _guard = self.log_lock.lock_or_recover();
            if let Err(error) = append_log(path, &record) {
                eprintln!(
                    "hotsheet-notify: could not write {}: {error}",
                    path.display()
                );
            }
        }
        record
    }
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

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn append_log(path: &Path, record: &DeliveryRecord) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(record).expect("serializable")
    )?;
    drop(file);
    let text = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > LOG_COMPACT_AT {
        let keep = lines[lines.len() - LOG_KEEP..].join("\n") + "\n";
        let tmp = path.with_extension(format!("{}.tmp", ulid::Ulid::new()));
        std::fs::write(&tmp, keep)?;
        std::fs::rename(&tmp, path)?;
    }
    Ok(())
}

/// The newest `limit` delivery records, newest first. Unparseable lines are skipped.
pub fn recent_deliveries(home: &Path, limit: usize) -> Vec<DeliveryRecord> {
    let Ok(text) = std::fs::read_to_string(home.join(LOG_FILE)) else {
        return Vec::new();
    };
    text.lines()
        .rev()
        .filter_map(|line| serde_json::from_str(line).ok())
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
        for i in 0..=LOG_COMPACT_AT {
            record.id = i.to_string();
            append_log(&path, &record).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap().lines().count(),
            LOG_KEEP
        );
        assert_eq!(
            recent_deliveries(home.path(), 1)[0].id,
            LOG_COMPACT_AT.to_string()
        );
    }
}
