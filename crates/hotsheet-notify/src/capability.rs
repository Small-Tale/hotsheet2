//! What a notifier can carry and how it is configured (`docs/32-notifier-plugins.md`
//! §32.3). Built-in and external notifiers share this descriptor, so a client renders a
//! settings form and the renderer degrades messages without knowing the notifier.

use serde::{Deserialize, Serialize};

/// A body representation, in the renderer's order of preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyFormat {
    Markdown,
    Html,
    Plain,
}

/// Attachment limits. `max_count == 0` means the notifier carries no attachments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentLimits {
    #[serde(default)]
    pub max_count: usize,
    /// Per-attachment byte cap.
    #[serde(default)]
    pub max_bytes: usize,
    /// Total bytes across every attachment of one message.
    #[serde(default)]
    pub max_total_bytes: usize,
    /// Only `image/*` attachments are carried.
    #[serde(default)]
    pub images_only: bool,
}

impl AttachmentLimits {
    pub const NONE: AttachmentLimits = AttachmentLimits {
        max_count: 0,
        max_bytes: 0,
        max_total_bytes: 0,
        images_only: false,
    };
}

impl Default for AttachmentLimits {
    fn default() -> Self {
        AttachmentLimits::NONE
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Accepted body formats; the renderer picks the first in [`BodyFormat`] order.
    pub formats: Vec<BodyFormat>,
    /// Title byte cap; 0 means the notifier has no separate title (it is folded into the body).
    #[serde(default)]
    pub max_title_bytes: usize,
    /// Body byte cap after rendering.
    pub max_body_bytes: usize,
    #[serde(default)]
    pub attachments: AttachmentLimits,
    /// The notifier maps severity to its own priority.
    #[serde(default)]
    pub priority: bool,
    /// The notifier can attach a click-through URL.
    #[serde(default)]
    pub click_url: bool,
    /// The output is spoken, so URLs and Markdown decoration are dropped.
    #[serde(default)]
    pub audio: bool,
}

/// The kind of value a setting holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum SettingKind {
    String,
    /// A list of strings (recipients, tags).
    List,
    Number,
    Bool,
    /// One of fixed choices.
    Choice {
        choices: Vec<String>,
    },
    /// A credential. Config stores only `{"secret": "<key name>"}`; the value lives in the
    /// machine key registry (OS keychain) and is resolved at delivery time.
    Secret,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingField {
    pub key: String,
    pub label: String,
    #[serde(flatten)]
    pub kind: SettingKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl SettingField {
    pub fn new(key: &str, label: &str, kind: SettingKind) -> SettingField {
        SettingField {
            key: key.into(),
            label: label.into(),
            kind,
            required: false,
            default: None,
            help: None,
        }
    }

    pub fn required(mut self) -> SettingField {
        self.required = true;
        self
    }

    pub fn default_value(mut self, value: serde_json::Value) -> SettingField {
        self.default = Some(value);
        self
    }

    pub fn help(mut self, help: &str) -> SettingField {
        self.help = Some(help.into());
        self
    }
}

/// Where a notifier came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum NotifierSource {
    BuiltIn,
    External { dir: String },
}

/// A notifier plugin's public description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotifierDescriptor {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub capabilities: Capabilities,
    #[serde(default)]
    pub settings: Vec<SettingField>,
    pub source: NotifierSource,
}
