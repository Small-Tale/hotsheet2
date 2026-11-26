//! The vocabulary of inbound remote-change hints (HS2-8X2S08, `docs/37`): what an adapter
//! reports, what a consumer is told, and how a connection's health reads.
//!
//! A hint is untrusted. It says "this Git remote may have advanced"; it never carries ticket
//! data and never changes anything by itself. A consumer reacts by checking the authoritative
//! remote.

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

/// The adapter contract version. An adapter built for another version is refused.
pub const CONTRACT_VERSION: u32 = 1;

/// Which repository a hint names. Either field may be absent in an event; a binding states
/// the identity it expects and a hint must agree with every field both sides have.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIdentity {
    /// The service's numeric repository id (stable across a rename).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// `owner/name`, compared case-insensitively.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
}

impl RepoIdentity {
    pub fn is_empty(&self) -> bool {
        self.id.is_none() && self.full_name.is_none()
    }

    /// Whether a hint's identity agrees with the identity a binding expects: nothing both
    /// sides state may differ, and at least one field must be stated by both, so a hint that
    /// names nothing never matches.
    pub fn matches(&self, expected: &RepoIdentity) -> bool {
        let mut compared = false;
        if let (Some(a), Some(b)) = (self.id, expected.id) {
            if a != b {
                return false;
            }
            compared = true;
        }
        if let (Some(a), Some(b)) = (&self.full_name, &expected.full_name) {
            if !a.eq_ignore_ascii_case(b) {
                return false;
            }
            compared = true;
        }
        compared
    }
}

/// One normalized hint: a ref of a repository may have moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeHint {
    pub repository: RepoIdentity,
    /// The full ref name, e.g. `refs/heads/main`.
    pub ref_name: String,
    /// The tip the event advertises (a 40 or 64 hex digit object id). A hint, not a fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advertised_tip: Option<String>,
    /// The delivery's identity, when the service gives one: the key duplicates collapse on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// When the event happened or arrived, milliseconds since the epoch.
    pub at_ms: u64,
}

/// What an adapter tells the hub while a connection runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The service accepted the connection and the channels are attached. `resumed` is
    /// `false` when events in the gap may have been missed, which asks every bound source
    /// for an authoritative check.
    Connected { resumed: bool },
    /// A hint arrived on a channel.
    Hint { channel: String, hint: ChangeHint },
}

/// Why a connection ended or a test failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdapterError {
    /// The credential was refused. Retrying cannot help until it changes.
    #[error("the service rejected the credential: {0}")]
    Auth(String),
    /// A network or protocol failure; reconnecting may work.
    #[error("{0}")]
    Transient(String),
    /// The configuration cannot work (a bad endpoint, a missing setting).
    #[error("{0}")]
    Config(String),
}

impl AdapterError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Auth(_) => "auth_failed",
            Self::Transient(_) => "unreachable",
            Self::Config(_) => "invalid_configuration",
        }
    }
}

/// What a plugin asks for in a connection's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub required: bool,
    pub help: &'static str,
}

/// What an adapter is, before any connection uses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdapterDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub contract: u32,
    /// Plain settings (never secrets) a connection may carry.
    pub settings: Vec<SettingSpec>,
    /// Whether the connection needs a credential in the OS credential store.
    pub credential: bool,
    /// What the credential should be, for the person who creates it.
    pub credential_help: &'static str,
}

/// Everything an adapter needs to open one connection. The credential is held here and
/// nowhere that prints.
pub struct ConnectionContext {
    pub connection: String,
    pub settings: BTreeMap<String, String>,
    pub credential: Option<hotsheet_sync::secret::SecretString>,
}

impl std::fmt::Debug for ConnectionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionContext")
            .field("connection", &self.connection)
            .field("settings", &self.settings)
            .field(
                "credential",
                &self.credential.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// A subscription adapter for one external event service. It runs blocking, on a thread the
/// hub owns, and reports through `emit`; it never retries on its own.
pub trait ChangeAdapter: Send + Sync {
    fn descriptor(&self) -> AdapterDescriptor;

    /// Open the connection, attach `channels`, and report events until `stop` is set (return
    /// `Ok`) or the connection fails (return the error). Emit [`Event::Connected`] once the
    /// channels are attached, and once more after any gap the adapter itself resumed over.
    fn run(
        &self,
        context: &ConnectionContext,
        channels: &[String],
        emit: &mut dyn FnMut(Event),
        stop: &AtomicBool,
    ) -> Result<(), AdapterError>;

    /// One cheap real check of the configuration and credential. Never subscribes for long.
    fn test(&self, context: &ConnectionContext) -> Result<(), AdapterError>;
}

/// A connection's typed health.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum Health {
    /// Nothing is bound to the connection, so nothing is running.
    Idle,
    Connecting,
    Connected {
        since_ms: u64,
    },
    /// The connection dropped; the hub is waiting `retry_in_ms` before it reconnects.
    Reconnecting {
        error: String,
        retry_in_ms: u64,
    },
    /// The credential was refused; the hub stops retrying until the connection is changed.
    AuthFailed {
        error: String,
    },
    /// The configuration cannot work; the hub stops until it changes.
    Misconfigured {
        error: String,
    },
}
