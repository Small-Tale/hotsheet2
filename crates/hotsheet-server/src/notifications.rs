use hotsheet_sync::LockExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::AppState;
use crate::events::ChangeEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Notification {
    pub id: String,
    pub message: String,
    pub severity: String,
    #[serde(default)]
    pub checkout: Option<String>,
    #[serde(default)]
    pub store: Option<String>,
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    pub created_at_ms: u64,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewNotification {
    pub message: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default)]
    pub checkout: Option<String>,
    #[serde(default)]
    pub store: Option<String>,
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default)]
    pub dedupe_key: Option<String>,
}
fn default_severity() -> String {
    "info".into()
}

#[derive(Clone, Default)]
pub struct NotificationHub {
    items: Arc<Mutex<Vec<Notification>>>,
}
impl NotificationHub {
    pub fn publish(&self, n: NewNotification) -> Notification {
        self.publish_with_status(n).0
    }

    fn publish_with_status(&self, n: NewNotification) -> (Notification, bool) {
        let mut items = self.items.lock_or_recover();
        if let Some(key) = &n.dedupe_key {
            if let Some(old) = items
                .iter()
                .rev()
                .find(|v| v.dedupe_key.as_ref() == Some(key) && !v.acknowledged)
            {
                return (old.clone(), false);
            }
        }
        let v = Notification {
            id: ulid::Ulid::new().to_string(),
            message: n.message,
            severity: n.severity,
            checkout: n.checkout,
            store: n.store,
            ticket: n.ticket,
            recipient: n.recipient,
            dedupe_key: n.dedupe_key,
            created_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            acknowledged: false,
        };
        items.push(v.clone());
        (v, true)
    }
    pub fn list(
        &self,
        checkout: Option<&str>,
        store: Option<&str>,
        ticket: Option<&str>,
        recipient: Option<&str>,
    ) -> Vec<Notification> {
        self.items
            .lock_or_recover()
            .iter()
            .filter(|n| {
                checkout.is_none_or(|v| n.checkout.as_deref() == Some(v))
                    && store.is_none_or(|v| n.store.as_deref() == Some(v))
                    && ticket.is_none_or(|v| n.ticket.as_deref() == Some(v))
                    && recipient.is_none_or(|v| n.recipient.as_deref() == Some(v))
            })
            .cloned()
            .collect()
    }
    pub fn acknowledge(&self, id: &str) -> Option<Notification> {
        let mut items = self.items.lock_or_recover();
        let n = items.iter_mut().find(|v| v.id == id)?;
        n.acknowledged = true;
        Some(n.clone())
    }
}

impl AppState {
    pub(crate) fn publish_notification(&self, body: NewNotification) -> Notification {
        let (notification, created) = self.notifications.publish_with_status(body);
        if created {
            self.emit(ChangeEvent {
                live: None,
                cursor: None,
                store: notification.store.clone().unwrap_or_default(),
                kind: "notification".into(),
                id: notification.id.clone(),
                slug: notification.ticket.clone().unwrap_or_default(),
                message: Some(notification.message.clone()),
                activity: None,
                assignment: None,
                turn: None,
                milestone: None,
            });
        }
        notification
    }

    /// Route a terminal's OSC 9 message through the same scoped inbox as other attention (HS2-ZP869N).
    pub(crate) fn publish_terminal_notification(&self, terminal_id: &str, message: String) {
        let store = crate::multistore::store_url_id(&self.store);
        let digest = Sha256::digest(message.as_bytes());
        self.publish_notification(NewNotification {
            message,
            severity: "info".into(),
            checkout: None,
            store: Some(store.clone()),
            ticket: None,
            recipient: None,
            dedupe_key: Some(format!("osc9:{store}:{terminal_id}:{digest:x}")),
        });
    }
}

/// HS2-EX594V: a multi-site project's home site became unavailable or came back. The
/// orchestration runtime decides when (debounced, once per transition); this publishes
/// it in the inbox like any other notification, so it reaches the app and every
/// notifier channel a `notification` rule routes.
pub(crate) struct ServerHomeNotices {
    pub(crate) state: AppState,
}

impl hotsheet_cli::orchestration::HomeNotices for ServerHomeNotices {
    fn notify(&self, notification: &hotsheet_cli::orchestration::HomeNotification) {
        self.state
            .publish_notification(home_notification(notification));
    }

    /// HS2-KXKA7X: a trigger's review, source outage or refused delivery, through the
    /// same inbox and notifier channels.
    fn notify_trigger(&self, notification: &hotsheet_cli::orchestration::TriggerNotification) {
        self.state.publish_notification(NewNotification {
            message: format!("{}\n{}", notification.title, notification.body),
            severity: notification.severity.into(),
            checkout: Some(notification.owner_id.clone()),
            store: None,
            ticket: None,
            recipient: None,
            dedupe_key: Some(notification.dedupe_key.clone()),
        });
    }
}

/// The inbox entry for a home-outage notification: its title on the first line, as
/// the notifier router reads it, and the explanation after.
pub(crate) fn home_notification(
    notification: &hotsheet_cli::orchestration::HomeNotification,
) -> NewNotification {
    NewNotification {
        message: format!("{}\n{}", notification.title, notification.body),
        severity: notification.severity.into(),
        checkout: Some(notification.owner_id.clone()),
        store: None,
        ticket: None,
        recipient: None,
        dedupe_key: Some(notification.dedupe_key.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_and_deduplicates() {
        let h = NotificationHub::default();
        let n = NewNotification {
            message: "done".into(),
            severity: "info".into(),
            checkout: Some("a".into()),
            store: None,
            ticket: None,
            recipient: None,
            dedupe_key: Some("x".into()),
        };
        let a = h.publish(n.clone());
        let b = h.publish(n);
        assert_eq!(a.id, b.id);
        assert!(a.created_at_ms > 0);
        assert_eq!(h.list(Some("a"), None, None, None).len(), 1);
        h.acknowledge(&a.id).unwrap();
        assert!(h.list(Some("a"), None, None, None)[0].acknowledged);
    }

    /// HS2-EX594V: a home-outage notification is an inbox entry for the orchestration
    /// project, titled on its first line, and the same outage is published once.
    #[test]
    fn hs2_ex594v_a_home_outage_is_one_inbox_entry_per_transition() {
        use hotsheet_cli::orchestration::HomeNotification;
        use hotsheet_ticketing::orchestration::HomeNoticeKind;
        let notification = HomeNotification {
            owner_id: "release-train-0123456789ab".into(),
            home_target_id: "api".into(),
            kind: HomeNoticeKind::Unavailable,
            severity: "warning",
            title: "Home site API is unavailable".into(),
            body: "Its machine cannot be reached (unreachable).".into(),
            dedupe_key: "home-outage:release-train-0123456789ab:api:1".into(),
        };
        let entry = home_notification(&notification);
        assert_eq!(
            entry.message.lines().next(),
            Some("Home site API is unavailable")
        );
        assert_eq!(entry.severity, "warning");
        assert_eq!(
            entry.checkout.as_deref(),
            Some("release-train-0123456789ab")
        );
        let hub = NotificationHub::default();
        let first = hub.publish(entry.clone());
        let again = hub.publish(entry);
        assert_eq!(first.id, again.id);
        let back = home_notification(&HomeNotification {
            kind: HomeNoticeKind::Returned,
            severity: "success",
            title: "Home site API is back".into(),
            dedupe_key: "home-returned:release-train-0123456789ab:api:2".into(),
            ..notification
        });
        assert_ne!(hub.publish(back).id, first.id);
        assert_eq!(
            hub.list(Some("release-train-0123456789ab"), None, None, None)
                .len(),
            2
        );
    }
}
