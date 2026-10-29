//! The one channel/rule administration workflow the server routes and the headless CLI
//! both call (AGENTS.md headless setup). Every operation is idempotent: re-running it
//! with the same input leaves the same configuration.

use std::path::Path;

use crate::capability::SettingKind;
use crate::config::{ChannelConfig, ConfigError, NotificationConfig, Rule};
use crate::dispatch::{DeliveryRecord, Dispatcher};
use crate::message::{EventKind, Message};
use crate::registry::Registry;
use crate::{managed_secret_name, managed_secret_prefix};

/// Writes and removes machine credentials. Implemented for the key registry.
pub trait SecretWriter {
    fn set_secret(&self, name: &str, value: &str) -> Result<(), String>;
    fn delete_secret(&self, name: &str) -> Result<bool, String>;
}

impl<S: hotsheet_ticketing::SecretStore> SecretWriter for hotsheet_ticketing::KeyRegistry<S> {
    fn set_secret(&self, name: &str, value: &str) -> Result<(), String> {
        self.set(name, value).map_err(|e| e.to_string())
    }

    fn delete_secret(&self, name: &str) -> Result<bool, String> {
        self.delete(name).map_err(|e| e.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Storage(String),
}

impl From<ConfigError> for AdminError {
    fn from(error: ConfigError) -> Self {
        match error {
            ConfigError::Invalid(message) => AdminError::Invalid(message),
            other => AdminError::Storage(other.to_string()),
        }
    }
}

/// Hold the configuration lock for one read-modify-write, so a CLI edit and a server edit
/// running at once never drop each other's change (HS2-YGNDET). The lock file lives beside
/// the configuration in the machine home.
fn lock(home: &Path) -> Result<hotsheet_ticketing::FileLock, AdminError> {
    let storage =
        |e: std::io::Error| AdminError::Storage(format!("locking the notification config: {e}"));
    std::fs::create_dir_all(home).map_err(storage)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join(".notifications.json.lock"))
        .map_err(storage)?;
    hotsheet_ticketing::FileLock::acquire(file).map_err(storage)
}

fn load(home: &Path) -> Result<NotificationConfig, AdminError> {
    Ok(NotificationConfig::load(home)?)
}

/// Create or replace a channel. Secret fields must already be references; use
/// [`set_channel_secret`] to store a value. Re-putting a channel with the same notifier
/// keeps the credential references it already has unless the new settings name them, so
/// editing other settings never drops a stored credential.
pub fn put_channel(
    home: &Path,
    registry: &Registry,
    mut channel: ChannelConfig,
) -> Result<ChannelConfig, AdminError> {
    let _lock = lock(home)?;
    let mut config = load(home)?;
    if let (Some(existing), Some(notifier)) = (
        config
            .channel(&channel.id)
            .filter(|c| c.notifier == channel.notifier),
        registry.get(&channel.notifier),
    ) {
        // Re-adding keeps the approval; only an explicit trust re-approves changed code.
        channel.plugin_digest = existing.plugin_digest.clone();
        for field in &notifier.descriptor().settings {
            if field.kind == SettingKind::Secret && !channel.settings.contains_key(&field.key) {
                if let Some(reference) = existing.settings.get(&field.key) {
                    channel
                        .settings
                        .insert(field.key.clone(), reference.clone());
                }
            }
        }
    }
    if channel.plugin_digest.is_none() {
        // Creating a channel for an external plugin approves the plugin as it is now.
        channel.plugin_digest = current_digest(registry, &channel.notifier)?;
    }
    config.upsert_channel(channel.clone());
    config.save(home, registry)?;
    Ok(channel)
}

fn current_digest(registry: &Registry, notifier: &str) -> Result<Option<String>, AdminError> {
    registry
        .get(notifier)
        .and_then(|n| n.content_digest())
        .transpose()
        .map_err(AdminError::Invalid)
}

/// Approve a channel's external notifier plugin as it is now (HS2-208RB1). Built-in
/// notifiers need no approval; trusting their channels is a no-op that succeeds.
pub fn trust_channel(
    home: &Path,
    registry: &Registry,
    channel: &str,
) -> Result<ChannelConfig, AdminError> {
    let _lock = lock(home)?;
    let mut config = load(home)?;
    let entry = config
        .channels
        .iter_mut()
        .find(|c| c.id == channel)
        .ok_or_else(|| AdminError::NotFound(format!("unknown channel '{channel}'")))?;
    entry.plugin_digest = current_digest(registry, &entry.notifier)?;
    let trusted = entry.clone();
    config.save(home, registry)?;
    Ok(trusted)
}

/// Whether each channel's external plugin still matches its approval: `None` for
/// built-ins, `Some(true)` when approved and unchanged.
pub fn trust_status(
    config: &NotificationConfig,
    registry: &Registry,
) -> Vec<(String, Option<bool>)> {
    config
        .channels
        .iter()
        .map(|c| {
            let current = registry.get(&c.notifier).and_then(|n| n.content_digest());
            let status = current.map(|current| {
                current
                    .ok()
                    .is_some_and(|d| c.plugin_digest.as_deref() == Some(d.as_str()))
            });
            (c.id.clone(), status)
        })
        .collect()
}

/// Store a credential for a channel's secret field in the key registry and point the
/// field at it. The value never reaches `notifications.json`.
pub fn set_channel_secret(
    home: &Path,
    registry: &Registry,
    secrets: &dyn SecretWriter,
    channel: &str,
    field: &str,
    value: &str,
) -> Result<(), AdminError> {
    let _lock = lock(home)?;
    if value.is_empty() {
        return Err(AdminError::Invalid("the credential value is empty".into()));
    }
    let mut config = load(home)?;
    let entry = config
        .channels
        .iter_mut()
        .find(|c| c.id == channel)
        .ok_or_else(|| AdminError::NotFound(format!("unknown channel '{channel}'")))?;
    let notifier = registry.get(&entry.notifier).ok_or_else(|| {
        AdminError::Invalid(format!("notifier '{}' is not installed", entry.notifier))
    })?;
    let is_secret = notifier
        .descriptor()
        .settings
        .iter()
        .any(|f| f.key == field && f.kind == SettingKind::Secret);
    if !is_secret {
        return Err(AdminError::Invalid(format!(
            "'{field}' is not a credential setting of {}",
            entry.notifier
        )));
    }
    let name = managed_secret_name(channel, field);
    secrets
        .set_secret(&name, value)
        .map_err(AdminError::Storage)?;
    entry
        .settings
        .insert(field.into(), serde_json::json!({ "secret": name }));
    config.save(home, registry)?;
    Ok(())
}

pub fn set_channel_enabled(
    home: &Path,
    registry: &Registry,
    channel: &str,
    enabled: bool,
) -> Result<(), AdminError> {
    let _lock = lock(home)?;
    let mut config = load(home)?;
    let entry = config
        .channels
        .iter_mut()
        .find(|c| c.id == channel)
        .ok_or_else(|| AdminError::NotFound(format!("unknown channel '{channel}'")))?;
    entry.enabled = enabled;
    config.save(home, registry)?;
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ChannelRemoval {
    pub removed: bool,
    pub removed_rules: Vec<String>,
    pub deleted_secrets: Vec<String>,
}

/// Remove a channel, rules left without a channel, and the credentials Hot Sheet stored
/// for it. Removing an absent channel succeeds with `removed: false`.
pub fn remove_channel(
    home: &Path,
    registry: &Registry,
    secrets: &dyn SecretWriter,
    channel: &str,
) -> Result<ChannelRemoval, AdminError> {
    let _lock = lock(home)?;
    let mut config = load(home)?;
    let managed: Vec<String> = config
        .channel(channel)
        .map(|c| {
            c.settings
                .values()
                .filter_map(|v| crate::settings::secret_reference(v).map(str::to_owned))
                .filter(|name| name.starts_with(&managed_secret_prefix(channel)))
                .collect()
        })
        .unwrap_or_default();
    let (removed, removed_rules) = config.remove_channel(channel);
    if !removed {
        return Ok(ChannelRemoval::default());
    }
    config.save(home, registry)?;
    let mut deleted_secrets = Vec::new();
    for name in managed {
        if secrets.delete_secret(&name).map_err(AdminError::Storage)? {
            deleted_secrets.push(name);
        }
    }
    Ok(ChannelRemoval {
        removed,
        removed_rules,
        deleted_secrets,
    })
}

pub fn put_rule(home: &Path, registry: &Registry, rule: Rule) -> Result<Rule, AdminError> {
    let _lock = lock(home)?;
    let mut config = load(home)?;
    config.upsert_rule(rule.clone());
    config.save(home, registry)?;
    Ok(rule)
}

pub fn remove_rule(home: &Path, registry: &Registry, rule: &str) -> Result<bool, AdminError> {
    let _lock = lock(home)?;
    let mut config = load(home)?;
    if !config.remove_rule(rule) {
        return Ok(false);
    }
    config.save(home, registry)?;
    Ok(true)
}

/// The fixed message `test` sends.
pub fn test_message(channel: &str) -> Message {
    let mut message = Message::new(EventKind::Test, "Hot Sheet test notification");
    message.body = format!(
        "This is a test from Hot Sheet for the **{channel}** channel. If you can read this, the channel works."
    );
    message
}

/// Deliver the test message to one channel, whether or not it is enabled.
pub fn test_channel(
    home: &Path,
    dispatcher: &Dispatcher,
    channel: &str,
) -> Result<DeliveryRecord, AdminError> {
    let config = load(home)?;
    dispatcher
        .deliver_to(&config, channel, &test_message(channel))
        .map_err(AdminError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::ntfy::Ntfy;
    use crate::config::RuleFilter;
    use crate::dispatch::Outcome;
    use crate::notifier::LiveRunner;
    use crate::settings::SecretLookup;
    use crate::testing::FakeHttp;
    use hotsheet_sync::LockExt;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MemorySecrets(Mutex<BTreeMap<String, String>>);

    impl SecretWriter for MemorySecrets {
        fn set_secret(&self, name: &str, value: &str) -> Result<(), String> {
            self.0.lock_or_recover().insert(name.into(), value.into());
            Ok(())
        }

        fn delete_secret(&self, name: &str) -> Result<bool, String> {
            Ok(self.0.lock_or_recover().remove(name).is_some())
        }
    }

    impl crate::settings::SecretLookup for MemorySecrets {
        fn secret(&self, name: &str) -> Result<String, String> {
            self.0
                .lock_or_recover()
                .get(name)
                .cloned()
                .ok_or_else(|| "missing".into())
        }
    }

    fn channel() -> ChannelConfig {
        ChannelConfig {
            id: "phone".into(),
            notifier: "ntfy".into(),
            label: Some("My phone".into()),
            enabled: true,
            settings: serde_json::from_value(json!({"topic":"hs-test"})).unwrap(),
            plugin_digest: None,
            quiet_hours: None,
            max_per_hour: None,
        }
    }

    #[test]
    fn channel_secret_rule_lifecycle_is_idempotent_and_cleans_up() {
        let home = tempfile::tempdir().unwrap();
        let registry = Registry::builtin_only();
        let secrets = MemorySecrets::default();
        put_channel(home.path(), &registry, channel()).unwrap();
        put_channel(home.path(), &registry, channel()).unwrap();
        set_channel_secret(home.path(), &registry, &secrets, "phone", "token", "tk").unwrap();
        // Re-adding the channel with new settings keeps its stored credential reference.
        let mut edited = channel();
        edited.settings.insert("priority".into(), json!("high"));
        let kept = put_channel(home.path(), &registry, edited).unwrap();
        assert_eq!(
            kept.settings["token"],
            json!({"secret":"notify-phone-token"})
        );
        let text = std::fs::read_to_string(NotificationConfig::path(home.path())).unwrap();
        assert!(
            !text.contains("\"tk\""),
            "secret values never reach the config file"
        );
        assert!(text.contains("notify-phone-token"));
        assert_eq!(secrets.secret("notify-phone-token").unwrap(), "tk");
        let err = set_channel_secret(home.path(), &registry, &secrets, "phone", "topic", "x")
            .unwrap_err();
        assert!(matches!(err, AdminError::Invalid(_)));
        assert!(matches!(
            set_channel_secret(home.path(), &registry, &secrets, "nope", "token", "x").unwrap_err(),
            AdminError::NotFound(_)
        ));
        let rule = Rule {
            id: "stopped".into(),
            events: vec![EventKind::ProjectStopped],
            channels: vec!["phone".into()],
            enabled: true,
            filter: RuleFilter::default(),
            cooldown_seconds: 900,
        };
        put_rule(home.path(), &registry, rule.clone()).unwrap();
        put_rule(home.path(), &registry, rule).unwrap();
        set_channel_enabled(home.path(), &registry, "phone", false).unwrap();
        let config = NotificationConfig::load(home.path()).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert!(!config.channels[0].enabled);

        let removal = remove_channel(home.path(), &registry, &secrets, "phone").unwrap();
        assert_eq!(removal.removed_rules, ["stopped"]);
        assert_eq!(removal.deleted_secrets, ["notify-phone-token"]);
        assert!(secrets.0.lock_or_recover().is_empty());
        assert_eq!(
            remove_channel(home.path(), &registry, &secrets, "phone").unwrap(),
            ChannelRemoval::default()
        );
        assert!(!remove_rule(home.path(), &registry, "stopped").unwrap());
    }

    #[test]
    fn hs2_ygndet_concurrent_writers_lose_no_change() {
        let home = tempfile::tempdir().unwrap();
        let registry = Arc::new(Registry::builtin_only());
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let home = home.path().to_path_buf();
                let registry = registry.clone();
                std::thread::spawn(move || {
                    let mut c = channel();
                    c.id = format!("phone-{i}");
                    put_channel(&home, &registry, c).unwrap();
                    put_rule(
                        &home,
                        &registry,
                        Rule {
                            id: format!("rule-{i}"),
                            events: vec![EventKind::Digest],
                            channels: vec![format!("phone-{i}")],
                            enabled: true,
                            filter: RuleFilter::default(),
                            cooldown_seconds: 0,
                        },
                    )
                    .unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let config = NotificationConfig::load(home.path()).unwrap();
        assert_eq!(config.channels.len(), 8);
        assert_eq!(config.rules.len(), 8);
    }

    #[test]
    fn hs2_208rb1_changed_plugins_are_refused_until_trusted_again() {
        use crate::notifier::ProcessOutput;
        use crate::testing::RecordingRunner;
        let home = tempfile::tempdir().unwrap();
        let plugin = Registry::plugins_dir(home.path()).join("relay");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join("manifest.toml"),
            "id = \"relay\"\ndisplay_name = \"Relay\"\ncommand = [\"send.sh\"]\n[capabilities]\nformats = [\"plain\"]\nmax_body_bytes = 500\n",
        )
        .unwrap();
        std::fs::write(plugin.join("send.sh"), "v1").unwrap();
        let runner = Arc::new(RecordingRunner::with_output(ProcessOutput {
            success: true,
            code: Some(0),
            stdout: br#"{"ok":true}"#.to_vec(),
            stderr: Vec::new(),
        }));
        let registry = || {
            Registry::with_builtins(
                Vec::new(),
                Some(&Registry::plugins_dir(home.path())),
                runner.clone(),
            )
        };
        let relay = ChannelConfig {
            id: "relay".into(),
            notifier: "relay".into(),
            label: None,
            enabled: true,
            settings: Default::default(),
            plugin_digest: None,
            quiet_hours: None,
            max_per_hour: None,
        };
        // Creating the channel approves the plugin as it is.
        let created = put_channel(home.path(), &registry(), relay.clone()).unwrap();
        assert!(created.plugin_digest.is_some());
        let dispatcher = || {
            Dispatcher::new(
                Arc::new(registry()),
                Arc::new(MemorySecrets::default()),
                Some(home.path()),
            )
        };
        let send = || test_channel(home.path(), &dispatcher(), "relay").unwrap();
        assert_eq!(send().outcome, Outcome::Delivered);
        assert_eq!(runner.calls().len(), 1);
        let config = NotificationConfig::load(home.path()).unwrap();
        assert_eq!(
            trust_status(&config, &registry()),
            [("relay".to_string(), Some(true))]
        );

        // A changed plugin is refused without running it.
        std::fs::write(plugin.join("send.sh"), "v2").unwrap();
        let refused = send();
        assert_eq!((refused.outcome, refused.attempts), (Outcome::Failed, 0));
        assert!(
            refused
                .error
                .unwrap()
                .contains("notify channel trust relay")
        );
        assert_eq!(runner.calls().len(), 1);
        assert_eq!(
            trust_status(&config, &registry()),
            [("relay".to_string(), Some(false))]
        );

        // Re-adding the channel does not re-approve; an explicit trust does.
        put_channel(home.path(), &registry(), relay.clone()).unwrap();
        assert_eq!(send().outcome, Outcome::Failed);
        let trusted = trust_channel(home.path(), &registry(), "relay").unwrap();
        assert_ne!(trusted.plugin_digest, created.plugin_digest);
        assert_eq!(send().outcome, Outcome::Delivered);
        assert_eq!(runner.calls().len(), 2);

        // A channel saved before approvals existed is refused until trusted.
        let mut config = NotificationConfig::load(home.path()).unwrap();
        config.channels[0].plugin_digest = None;
        config.save(home.path(), &registry()).unwrap();
        assert!(send().error.unwrap().contains("has not approved"));
        assert!(matches!(
            trust_channel(home.path(), &registry(), "nope").unwrap_err(),
            AdminError::NotFound(_)
        ));
    }

    #[test]
    fn invalid_input_is_rejected_without_writing() {
        let home = tempfile::tempdir().unwrap();
        let registry = Registry::builtin_only();
        let mut bad = channel();
        bad.settings.insert("token".into(), json!("inline-secret"));
        assert!(matches!(
            put_channel(home.path(), &registry, bad).unwrap_err(),
            AdminError::Invalid(_)
        ));
        assert!(!NotificationConfig::path(home.path()).exists());
        let rule = Rule {
            id: "r".into(),
            events: vec![EventKind::Digest],
            channels: vec!["missing".into()],
            enabled: true,
            filter: RuleFilter::default(),
            cooldown_seconds: 0,
        };
        assert!(matches!(
            put_rule(home.path(), &registry, rule).unwrap_err(),
            AdminError::Invalid(_)
        ));
    }

    #[test]
    fn test_channel_sends_the_fixed_message_even_when_disabled() {
        let home = tempfile::tempdir().unwrap();
        let http = Arc::new(FakeHttp::replying(200, "{}"));
        let registry = Arc::new(Registry::with_builtins(
            vec![Arc::new(Ntfy::with_transport(http.clone()))],
            None,
            Arc::new(LiveRunner),
        ));
        let mut disabled = channel();
        disabled.enabled = false;
        put_channel(home.path(), &registry, disabled).unwrap();
        let dispatcher = Dispatcher::new(
            registry,
            Arc::new(MemorySecrets::default()),
            Some(home.path()),
        );
        let record = test_channel(home.path(), &dispatcher, "phone").unwrap();
        assert_eq!(
            (record.outcome, record.event),
            (Outcome::Delivered, EventKind::Test)
        );
        let request = http.requests().remove(0);
        assert!(
            String::from_utf8(request.body)
                .unwrap()
                .contains("**phone** channel")
        );
        assert!(matches!(
            test_channel(home.path(), &dispatcher, "nope").unwrap_err(),
            AdminError::NotFound(_)
        ));
    }
}
