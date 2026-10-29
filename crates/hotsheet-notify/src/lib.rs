//! Notifier plugins (`docs/32-notifier-plugins.md`, HS2-R5HTZN): a provider-neutral
//! notification [`Message`], per-notifier [`Capabilities`] the renderer degrades to,
//! built-in `speak`, `ntfy`, and `email` notifiers, external subprocess notifiers, the
//! machine-local channel and rule configuration, and the dispatcher that routes events.
//!
//! Both the server and the headless CLI drive this crate, so every channel can be set up,
//! tested, and used without the graphical client.

pub mod admin;
pub mod builtin;
pub mod capability;
pub mod config;
pub mod dispatch;
pub mod external;
pub mod http;
pub mod message;
pub mod notifier;
pub mod registry;
pub mod render;
pub mod settings;
pub mod testing;

pub use capability::{BodyFormat, Capabilities, NotifierDescriptor, SettingField, SettingKind};
pub use config::{ChannelConfig, ConfigError, NotificationConfig, Rule, RuleFilter};
pub use dispatch::{DeliveryRecord, Dispatcher, Outcome, recent_deliveries};
pub use message::{Attachment, EventKind, Message, Severity, TicketFacts};
pub use notifier::{DeliveryError, Notifier};
pub use registry::Registry;
pub use settings::SecretLookup;

/// The key-registry name for a channel's credential field, as `notify channel add` stores
/// it. Removing the channel deletes keys under this prefix.
pub fn managed_secret_name(channel: &str, field: &str) -> String {
    format!("notify-{channel}-{field}")
}

pub fn managed_secret_prefix(channel: &str) -> String {
    format!("notify-{channel}-")
}
