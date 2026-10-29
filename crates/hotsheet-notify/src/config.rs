//! `${HOTSHEET_HOME}/notifications.json`: configured channels and routing rules
//! (`docs/32-notifier-plugins.md` §32.5). Machine-local and secret-free: credential
//! settings hold key-registry references only.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::external::valid_id;
use crate::message::{EventKind, Message, Severity};
use crate::registry::Registry;
use crate::settings::{self, Settings};

pub const CONFIG_FILE: &str = "notifications.json";
pub const CONFIG_VERSION: u32 = 1;

fn yes() -> bool {
    true
}

/// A daily window during which a channel holds deliveries (HS2-4JG95G). Times are
/// `HH:MM` in the fixed `utc_offset` (`+09:00`, `-05:00`); a window may wrap midnight.
/// A fixed offset avoids local-time lookups in a multithreaded server; adjust it when
/// daylight saving time changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    pub start: String,
    pub end: String,
    #[serde(default = "utc")]
    pub utc_offset: String,
    /// Error-severity messages (a stalled project, a failed build) still go out.
    #[serde(default = "yes")]
    pub allow_errors: bool,
}

fn utc() -> String {
    "+00:00".into()
}

fn minutes(hhmm: &str) -> Option<i64> {
    let (h, m) = hhmm.split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m) && hhmm.len() == 5).then_some(h * 60 + m)
}

fn offset(text: &str) -> Option<time::UtcOffset> {
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let total = minutes(&text[1..])?;
    time::UtcOffset::from_whole_seconds((sign * total * 60) as i32).ok()
}

impl QuietHours {
    pub fn validate(&self) -> Result<(), String> {
        let (Some(start), Some(end)) = (minutes(&self.start), minutes(&self.end)) else {
            return Err("quiet hours start and end must be HH:MM".into());
        };
        if start == end {
            return Err("quiet hours start and end must differ".into());
        }
        offset(&self.utc_offset)
            .map(|_| ())
            .ok_or_else(|| "quiet hours utc_offset must look like +09:00 or -05:00".into())
    }

    /// Whether `at` falls inside the window.
    pub fn contains(&self, at: time::OffsetDateTime) -> bool {
        let (Some(start), Some(end), Some(offset)) = (
            minutes(&self.start),
            minutes(&self.end),
            offset(&self.utc_offset),
        ) else {
            return false;
        };
        let local = at.to_offset(offset);
        let now = i64::from(local.hour()) * 60 + i64::from(local.minute());
        if start < end {
            (start..end).contains(&now)
        } else {
            now >= start || now < end
        }
    }
}

/// One configured destination: a notifier plus its settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub id: String,
    pub notifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub settings: Settings,
    /// For an external notifier: the plugin digest this channel approved. Delivery is
    /// refused when the plugin's current digest differs (HS2-208RB1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<QuietHours>,
    /// At most this many deliveries per rolling hour; later ones are suppressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_hour: Option<u32>,
}

/// Narrows which events a rule forwards. Empty lists match everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleFilter {
    /// Checkout ids (or store ids) the event must belong to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
    /// The ticket must carry at least one of these tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The ticket's (new) status must be one of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statuses: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub priorities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_severity: Option<Severity>,
    /// Case-insensitive substring of the ticket's slug, title, or a tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Routes matching events to channels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Defaulted so an HTTP body may omit it; the route takes the id from its path.
    #[serde(default)]
    pub id: String,
    pub events: Vec<EventKind>,
    pub channels: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_default_filter")]
    pub filter: RuleFilter,
    /// Repeats of a message with the same dedupe key inside this window are suppressed per
    /// channel. 0 disables suppression.
    #[serde(default)]
    pub cooldown_seconds: u64,
}

fn is_default_filter(filter: &RuleFilter) -> bool {
    filter == &RuleFilter::default()
}

fn any_eq(wanted: &[String], actual: Option<&str>) -> bool {
    wanted.is_empty() || actual.is_some_and(|a| wanted.iter().any(|w| w.eq_ignore_ascii_case(a)))
}

impl Rule {
    pub fn matches(&self, message: &Message) -> bool {
        if !self.enabled || !self.events.contains(&message.event) {
            return false;
        }
        let f = &self.filter;
        if f.min_severity.is_some_and(|min| message.severity < min) {
            return false;
        }
        if !f.projects.is_empty()
            && !f.projects.iter().any(|p| {
                message.project.as_deref() == Some(p.as_str())
                    || message.store.as_deref() == Some(p.as_str())
                    || message.checkouts.iter().any(|c| c == p)
            })
        {
            return false;
        }
        let text = f
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_lowercase);
        let ticket_filters = !(f.tags.is_empty()
            && f.statuses.is_empty()
            && f.categories.is_empty()
            && f.priorities.is_empty()
            && text.is_none());
        if !ticket_filters {
            return true;
        }
        let Some(ticket) = &message.ticket else {
            return false;
        };
        (f.tags.is_empty()
            || ticket
                .tags
                .iter()
                .any(|t| f.tags.iter().any(|w| w.eq_ignore_ascii_case(t))))
            && any_eq(&f.statuses, ticket.status.as_deref())
            && any_eq(&f.categories, ticket.category.as_deref())
            && any_eq(&f.priorities, ticket.priority.as_deref())
            && text.is_none_or(|needle| {
                ticket.slug.to_lowercase().contains(&needle)
                    || ticket
                        .title
                        .as_deref()
                        .is_some_and(|t| t.to_lowercase().contains(&needle))
                    || ticket
                        .tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&needle))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationConfig {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// The client origin click-through links use (HS2-04J8WS); a LAN or public origin
    /// when notifications are read on another device. Unset: the local client's origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_base: Option<String>,
}

fn version() -> u32 {
    CONFIG_VERSION
}

impl Default for NotificationConfig {
    fn default() -> Self {
        NotificationConfig {
            version: CONFIG_VERSION,
            channels: Vec::new(),
            rules: Vec::new(),
            link_base: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0}")]
    Invalid(String),
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("parsing {path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
}

fn invalid(message: impl Into<String>) -> ConfigError {
    ConfigError::Invalid(message.into())
}

impl NotificationConfig {
    pub fn path(home: &Path) -> PathBuf {
        home.join(CONFIG_FILE)
    }

    pub fn load(home: &Path) -> Result<NotificationConfig, ConfigError> {
        let path = NotificationConfig::path(home);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let config: NotificationConfig =
                    serde_json::from_str(&text).map_err(|source| ConfigError::Parse {
                        path: path.display().to_string(),
                        source,
                    })?;
                if config.version > CONFIG_VERSION {
                    return Err(invalid(format!(
                        "{} is version {}; this Hot Sheet reads version {CONFIG_VERSION}",
                        path.display(),
                        config.version
                    )));
                }
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NotificationConfig::default()),
            Err(source) => Err(ConfigError::Io {
                path: path.display().to_string(),
                source,
            }),
        }
    }

    /// Validate, then write atomically (temp file + rename).
    pub fn save(&self, home: &Path, registry: &Registry) -> Result<(), ConfigError> {
        self.validate(registry)?;
        let path = NotificationConfig::path(home);
        let io = |source| ConfigError::Io {
            path: path.display().to_string(),
            source,
        };
        std::fs::create_dir_all(home).map_err(io)?;
        let tmp = home.join(format!(".{CONFIG_FILE}.{}.tmp", ulid::Ulid::new()));
        let text = serde_json::to_string_pretty(self).expect("serializable") + "\n";
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            io(e)
        })
    }

    pub fn validate(&self, registry: &Registry) -> Result<(), ConfigError> {
        let mut seen = std::collections::BTreeSet::new();
        for channel in &self.channels {
            if !valid_id(&channel.id) {
                return Err(invalid(format!(
                    "channel id '{}' must be lowercase letters, digits, '-' or '_'",
                    channel.id
                )));
            }
            if !seen.insert(channel.id.as_str()) {
                return Err(invalid(format!("duplicate channel '{}'", channel.id)));
            }
            let notifier = registry.get(&channel.notifier).ok_or_else(|| {
                invalid(format!(
                    "channel '{}': unknown notifier '{}'",
                    channel.id, channel.notifier
                ))
            })?;
            settings::validate(&notifier.descriptor().settings, &channel.settings)
                .map_err(|e| invalid(format!("channel '{}': {e}", channel.id)))?;
            if let Some(quiet) = &channel.quiet_hours {
                quiet
                    .validate()
                    .map_err(|e| invalid(format!("channel '{}': {e}", channel.id)))?;
            }
            if channel.max_per_hour == Some(0) {
                return Err(invalid(format!(
                    "channel '{}': max_per_hour must be at least 1",
                    channel.id
                )));
            }
        }
        if let Some(base) = &self.link_base {
            if !(base.starts_with("http://") || base.starts_with("https://")) {
                return Err(invalid("link_base must be an http(s) origin"));
            }
        }
        let mut rules = std::collections::BTreeSet::new();
        for rule in &self.rules {
            if !valid_id(&rule.id) {
                return Err(invalid(format!(
                    "rule id '{}' must be lowercase letters, digits, '-' or '_'",
                    rule.id
                )));
            }
            if !rules.insert(rule.id.as_str()) {
                return Err(invalid(format!("duplicate rule '{}'", rule.id)));
            }
            if rule.events.is_empty() {
                return Err(invalid(format!(
                    "rule '{}' must name at least one event",
                    rule.id
                )));
            }
            if rule.events.contains(&EventKind::Test) {
                return Err(invalid(format!(
                    "rule '{}': test notifications go only to the channel being tested",
                    rule.id
                )));
            }
            if rule.channels.is_empty() {
                return Err(invalid(format!(
                    "rule '{}' must name at least one channel",
                    rule.id
                )));
            }
            for channel in &rule.channels {
                if !seen.contains(channel.as_str()) {
                    return Err(invalid(format!(
                        "rule '{}': unknown channel '{channel}'",
                        rule.id
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn channel(&self, id: &str) -> Option<&ChannelConfig> {
        self.channels.iter().find(|c| c.id == id)
    }

    /// Insert or replace a channel by id.
    pub fn upsert_channel(&mut self, channel: ChannelConfig) {
        match self.channels.iter_mut().find(|c| c.id == channel.id) {
            Some(existing) => *existing = channel,
            None => self.channels.push(channel),
        }
    }

    /// Remove a channel and its references; rules left with no channel are removed too.
    /// Returns whether the channel existed and the ids of removed rules.
    pub fn remove_channel(&mut self, id: &str) -> (bool, Vec<String>) {
        let before = self.channels.len();
        self.channels.retain(|c| c.id != id);
        let mut removed_rules = Vec::new();
        for rule in &mut self.rules {
            rule.channels.retain(|c| c != id);
            if rule.channels.is_empty() {
                removed_rules.push(rule.id.clone());
            }
        }
        self.rules.retain(|r| !r.channels.is_empty());
        (self.channels.len() != before, removed_rules)
    }

    pub fn upsert_rule(&mut self, rule: Rule) {
        match self.rules.iter_mut().find(|r| r.id == rule.id) {
            Some(existing) => *existing = rule,
            None => self.rules.push(rule),
        }
    }

    pub fn remove_rule(&mut self, id: &str) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        self.rules.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::TicketFacts;
    use serde_json::json;

    fn registry() -> Registry {
        Registry::builtin_only()
    }

    fn ntfy(id: &str) -> ChannelConfig {
        ChannelConfig {
            id: id.into(),
            notifier: "ntfy".into(),
            label: None,
            enabled: true,
            settings: serde_json::from_value(json!({"topic":"hs"})).unwrap(),
            plugin_digest: None,
            quiet_hours: None,
            max_per_hour: None,
        }
    }

    fn rule(events: Vec<EventKind>) -> Rule {
        Rule {
            id: "r".into(),
            events,
            channels: vec!["phone".into()],
            enabled: true,
            filter: RuleFilter::default(),
            cooldown_seconds: 0,
        }
    }

    fn ticket_message(status: &str, tags: &[&str]) -> Message {
        let mut m = Message::new(EventKind::TicketStatusChanged, "t");
        m.project = Some("hotsheet2-abc".into());
        m.ticket = Some(TicketFacts {
            slug: "HS2-1".into(),
            status: Some(status.into()),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            category: Some("bug".into()),
            priority: Some("high".into()),
            ..TicketFacts::default()
        });
        m
    }

    #[test]
    fn rule_matching_by_event_project_ticket_and_severity() {
        let mut r = rule(vec![EventKind::TicketStatusChanged]);
        assert!(r.matches(&ticket_message("completed", &[])));
        assert!(!r.matches(&Message::new(EventKind::TicketBlocked, "x")));
        r.filter.statuses = vec!["Completed".into()];
        r.filter.tags = vec!["release".into()];
        assert!(r.matches(&ticket_message("completed", &["release", "x"])));
        assert!(!r.matches(&ticket_message("completed", &["x"])));
        assert!(!r.matches(&ticket_message("started", &["release"])));
        let mut no_ticket = ticket_message("completed", &["release"]);
        no_ticket.ticket = None;
        assert!(!r.matches(&no_ticket));
        r.filter = RuleFilter {
            projects: vec!["other".into()],
            ..RuleFilter::default()
        };
        assert!(!r.matches(&ticket_message("completed", &[])));
        r.filter.projects = vec!["hotsheet2-abc".into()];
        r.filter.categories = vec!["bug".into()];
        r.filter.priorities = vec!["high".into()];
        assert!(r.matches(&ticket_message("completed", &[])));
        r.filter.min_severity = Some(Severity::Warning);
        assert!(!r.matches(&ticket_message("completed", &[])));
        r.enabled = false;
        r.filter = RuleFilter::default();
        assert!(!r.matches(&ticket_message("completed", &[])));
    }

    #[test]
    fn hs2_4jg95g_text_and_checkout_filters() {
        let mut r = rule(vec![EventKind::TicketStatusChanged]);
        r.filter.text = Some("  Release ".into());
        let mut m = ticket_message("completed", &[]);
        assert!(!r.matches(&m));
        m.ticket.as_mut().unwrap().title = Some("Cut the release branch".into());
        assert!(r.matches(&m), "title match");
        m.ticket.as_mut().unwrap().title = None;
        m.ticket.as_mut().unwrap().tags = vec!["pre-release".into()];
        assert!(r.matches(&m), "tag substring match");
        r.filter.text = Some("hs2-1".into());
        assert!(r.matches(&m), "slug match, case-insensitive");
        r.filter.text = Some("   ".into());
        assert!(r.matches(&m), "blank text does not filter");
        let mut by_checkout = rule(vec![EventKind::TicketStatusChanged]);
        by_checkout.filter.projects = vec!["hotsheet2-ab12".into()];
        assert!(!by_checkout.matches(&m));
        m.checkouts = vec!["other-1".into(), "hotsheet2-ab12".into()];
        assert!(by_checkout.matches(&m));
    }

    #[test]
    fn hs2_4jg95g_quiet_hours_windows() {
        let at = |h: u8, m: u8| {
            time::OffsetDateTime::UNIX_EPOCH.replace_time(time::Time::from_hms(h, m, 0).unwrap())
        };
        let night = QuietHours {
            start: "22:00".into(),
            end: "07:00".into(),
            utc_offset: "+00:00".into(),
            allow_errors: true,
        };
        assert!(night.validate().is_ok());
        assert!(
            night.contains(at(23, 30)) && night.contains(at(6, 59)) && night.contains(at(22, 0))
        );
        assert!(!night.contains(at(7, 0)) && !night.contains(at(12, 0)));
        let lunch = QuietHours {
            start: "12:00".into(),
            end: "13:00".into(),
            utc_offset: "+09:00".into(),
            allow_errors: false,
        };
        // 03:30 UTC is 12:30 at +09:00.
        assert!(lunch.contains(at(3, 30)) && !lunch.contains(at(12, 30)));
        for bad in [
            ("25:00", "07:00", "+00:00"),
            ("22:00", "22:00", "+00:00"),
            ("22:00", "7:00", "+00:00"),
            ("22:00", "07:00", "09:00"),
        ] {
            let q = QuietHours {
                start: bad.0.into(),
                end: bad.1.into(),
                utc_offset: bad.2.into(),
                allow_errors: true,
            };
            assert!(q.validate().is_err(), "{bad:?}");
        }
        let reg = registry();
        let mut c = NotificationConfig::default();
        let mut phone = ntfy("phone");
        phone.quiet_hours = Some(QuietHours {
            start: "x".into(),
            ..night.clone()
        });
        c.channels.push(phone.clone());
        assert!(c.validate(&reg).unwrap_err().to_string().contains("HH:MM"));
        c.channels[0].quiet_hours = Some(night);
        c.channels[0].max_per_hour = Some(0);
        assert!(
            c.validate(&reg)
                .unwrap_err()
                .to_string()
                .contains("max_per_hour")
        );
    }

    #[test]
    fn validation_rejects_bad_references_and_ids() {
        let reg = registry();
        let mut c = NotificationConfig::default();
        c.channels.push(ntfy("phone"));
        c.rules.push(rule(vec![EventKind::TicketBlocked]));
        assert!(c.validate(&reg).is_ok());
        let check = |mutate: &dyn Fn(&mut NotificationConfig), expect: &str| {
            let mut bad = c.clone();
            mutate(&mut bad);
            let err = bad.validate(&reg).unwrap_err().to_string();
            assert!(err.contains(expect), "{err} !~ {expect}");
        };
        check(&|c| c.channels.push(ntfy("phone")), "duplicate channel");
        check(&|c| c.channels[0].id = "Phone".into(), "lowercase");
        check(
            &|c| c.channels[0].notifier = "pager".into(),
            "unknown notifier",
        );
        check(&|c| c.channels[0].settings.clear(), "'topic' is required");
        check(
            &|c| c.rules[0].channels = vec!["nope".into()],
            "unknown channel",
        );
        check(&|c| c.rules[0].channels.clear(), "at least one channel");
        check(&|c| c.rules[0].events.clear(), "at least one event");
        check(
            &|c| c.rules[0].events = vec![EventKind::Test],
            "test notifications",
        );
        check(
            &|c| c.rules.push(rule(vec![EventKind::Digest])),
            "duplicate rule",
        );
    }

    #[test]
    fn save_load_round_trip_and_newer_versions_refused() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            NotificationConfig::load(home.path()).unwrap(),
            NotificationConfig::default()
        );
        let mut c = NotificationConfig::default();
        c.upsert_channel(ntfy("phone"));
        c.upsert_rule(rule(vec![EventKind::ProjectStopped]));
        c.save(home.path(), &registry()).unwrap();
        assert_eq!(NotificationConfig::load(home.path()).unwrap(), c);
        let text = std::fs::read_to_string(NotificationConfig::path(home.path())).unwrap();
        assert!(text.contains("\"project.stopped\""));
        std::fs::write(NotificationConfig::path(home.path()), r#"{"version":99}"#).unwrap();
        assert!(
            NotificationConfig::load(home.path())
                .unwrap_err()
                .to_string()
                .contains("version 99")
        );
        std::fs::write(NotificationConfig::path(home.path()), "{").unwrap();
        assert!(matches!(
            NotificationConfig::load(home.path()),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn upsert_and_remove_keep_references_consistent() {
        let mut c = NotificationConfig::default();
        c.upsert_channel(ntfy("phone"));
        c.upsert_channel(ntfy("desk"));
        let mut replaced = ntfy("phone");
        replaced.enabled = false;
        c.upsert_channel(replaced);
        assert_eq!(c.channels.len(), 2);
        assert!(!c.channel("phone").unwrap().enabled);
        let mut both = rule(vec![EventKind::TicketBlocked]);
        both.id = "both".into();
        both.channels = vec!["phone".into(), "desk".into()];
        c.upsert_rule(both);
        c.upsert_rule(rule(vec![EventKind::Digest]));
        let (existed, removed) = c.remove_channel("phone");
        assert!(existed);
        assert_eq!(removed, ["r"]);
        assert_eq!(c.rules.len(), 1);
        assert_eq!(c.rules[0].channels, ["desk"]);
        assert!(!c.remove_channel("phone").0);
        assert!(c.remove_rule("both") && !c.remove_rule("both"));
    }
}
