//! The provider-neutral notification a Hot Sheet event produces, before any channel
//! renders it (`docs/32-notifier-plugins.md` §32.2).

use serde::{Deserialize, Serialize};

/// What happened. Rules subscribe to these; a channel never sees an unknown kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EventKind {
    /// A ticket's status changed (rules may narrow by the new status).
    #[serde(rename = "ticket.status_changed")]
    TicketStatusChanged,
    /// A ticket gained an open FEEDBACK NEEDED request.
    #[serde(rename = "ticket.feedback_needed")]
    TicketFeedbackNeeded,
    /// A ticket became blocked.
    #[serde(rename = "ticket.blocked")]
    TicketBlocked,
    /// An AI tool asked for a permission decision.
    #[serde(rename = "permission.requested")]
    PermissionRequested,
    /// A targeted in-app notification was published (HS2-ZP869N).
    #[serde(rename = "notification")]
    Notification,
    /// A periodic activity digest (HS2-D33WPH).
    #[serde(rename = "digest")]
    Digest,
    /// A project's AI work stopped or stalled (HS2-2MM07G).
    #[serde(rename = "project.stopped")]
    ProjectStopped,
    /// A user-initiated channel test; delivered only to the named channel.
    #[serde(rename = "test")]
    Test,
}

impl EventKind {
    pub const ALL: [EventKind; 8] = [
        EventKind::TicketStatusChanged,
        EventKind::TicketFeedbackNeeded,
        EventKind::TicketBlocked,
        EventKind::PermissionRequested,
        EventKind::Notification,
        EventKind::Digest,
        EventKind::ProjectStopped,
        EventKind::Test,
    ];

    /// The wire name (`ticket.status_changed`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::TicketStatusChanged => "ticket.status_changed",
            EventKind::TicketFeedbackNeeded => "ticket.feedback_needed",
            EventKind::TicketBlocked => "ticket.blocked",
            EventKind::PermissionRequested => "permission.requested",
            EventKind::Notification => "notification",
            EventKind::Digest => "digest",
            EventKind::ProjectStopped => "project.stopped",
            EventKind::Test => "test",
        }
    }

    pub fn parse(value: &str) -> Option<EventKind> {
        EventKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }
}

/// Ordered urgency. Channels map it to their own priority scale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    /// Parse the hub's free-form severity string; unknown values are `Info`.
    pub fn parse_lenient(value: &str) -> Severity {
        match value.trim().to_ascii_lowercase().as_str() {
            "success" => Severity::Success,
            "warning" | "warn" => Severity::Warning,
            "error" | "critical" | "danger" => Severity::Error,
            _ => Severity::Info,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Success => "success",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// Ticket facts a rule can filter on. Identifiers and labels only; never note text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketFacts {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub previous_status: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Saved views a rule names that the ticket is in, filled by the process that noticed the
    /// change (HS2-G8NXNC).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<crate::config::ViewRef>,
}

/// One binary attachment. Rendering may drop it when a channel cannot carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub content_type: String,
    #[serde(with = "base64_bytes")]
    pub bytes: Vec<u8>,
}

impl Attachment {
    pub fn is_image(&self) -> bool {
        self.content_type.starts_with("image/")
    }
}

/// A notification before rendering. `body` is CommonMark; channels without Markdown get
/// a plain-text or HTML rendering of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub event: EventKind,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub severity: Severity,
    /// The project (checkout id) the event belongs to, for rule filtering and routing.
    #[serde(default)]
    pub project: Option<String>,
    /// Human-readable project name used in copy ("Hot Sheet project <name> …").
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub store: Option<String>,
    #[serde(default)]
    pub ticket: Option<TicketFacts>,
    /// Where a click should go, when the channel supports links.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// Repeats with the same key inside a rule's cooldown are suppressed per channel.
    #[serde(default)]
    pub dedupe_key: Option<String>,
    /// Checkout ids linked to the event's store, so a rule's `projects` filter can name a
    /// checkout for store-scoped events (HS2-4JG95G).
    #[serde(default)]
    pub checkouts: Vec<String>,
}

impl Message {
    pub fn new(event: EventKind, title: impl Into<String>) -> Message {
        Message {
            event,
            title: title.into(),
            body: String::new(),
            severity: Severity::Info,
            project: None,
            project_name: None,
            store: None,
            ticket: None,
            url: None,
            attachments: Vec::new(),
            dedupe_key: None,
            checkouts: Vec::new(),
        }
    }
}

mod base64_bytes {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kinds_round_trip_by_wire_name() {
        for kind in EventKind::ALL {
            assert_eq!(EventKind::parse(kind.as_str()), Some(kind));
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
        }
        assert_eq!(EventKind::parse("ticket.unknown"), None);
    }

    #[test]
    fn severity_parses_hub_strings_and_orders() {
        assert_eq!(Severity::parse_lenient("WARN"), Severity::Warning);
        assert_eq!(Severity::parse_lenient("critical"), Severity::Error);
        assert_eq!(Severity::parse_lenient("whatever"), Severity::Info);
        assert!(Severity::Error > Severity::Warning && Severity::Warning > Severity::Info);
    }

    #[test]
    fn attachments_serialize_as_base64() {
        let a = Attachment {
            name: "a.png".into(),
            content_type: "image/png".into(),
            bytes: vec![1, 2, 3],
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["bytes"], "AQID");
        assert_eq!(serde_json::from_value::<Attachment>(json).unwrap(), a);
        assert!(a.is_image());
    }
}
