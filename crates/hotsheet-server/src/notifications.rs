use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

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
    pub dedupe_key: Option<String>,
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
        let mut items = self.items.lock().unwrap();
        if let Some(key) = &n.dedupe_key {
            if let Some(old) = items
                .iter()
                .rev()
                .find(|v| v.dedupe_key.as_ref() == Some(key) && !v.acknowledged)
            {
                return old.clone();
            }
        }
        let v = Notification {
            id: ulid::Ulid::new().to_string(),
            message: n.message,
            severity: n.severity,
            checkout: n.checkout,
            store: n.store,
            ticket: n.ticket,
            dedupe_key: n.dedupe_key,
            acknowledged: false,
        };
        items.push(v.clone());
        v
    }
    pub fn list(
        &self,
        checkout: Option<&str>,
        store: Option<&str>,
        ticket: Option<&str>,
    ) -> Vec<Notification> {
        self.items
            .lock()
            .unwrap()
            .iter()
            .filter(|n| {
                checkout.is_none_or(|v| n.checkout.as_deref() == Some(v))
                    && store.is_none_or(|v| n.store.as_deref() == Some(v))
                    && ticket.is_none_or(|v| n.ticket.as_deref() == Some(v))
            })
            .cloned()
            .collect()
    }
    pub fn acknowledge(&self, id: &str) -> Option<Notification> {
        let mut items = self.items.lock().unwrap();
        let n = items.iter_mut().find(|v| v.id == id)?;
        n.acknowledged = true;
        Some(n.clone())
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
            dedupe_key: Some("x".into()),
        };
        let a = h.publish(n.clone());
        let b = h.publish(n);
        assert_eq!(a.id, b.id);
        assert_eq!(h.list(Some("a"), None, None).len(), 1);
        h.acknowledge(&a.id).unwrap();
        assert!(h.list(Some("a"), None, None)[0].acknowledged);
    }
}
