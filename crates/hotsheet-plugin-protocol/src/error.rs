//! Structured plugin errors (docs/38 §38.5). Codes are strings so JSON plugin authors
//! can write them directly; the host maps them onto its own error types.

use crate::v1::PluginError;

/// The error codes a plugin or the host may report.
pub mod code {
    pub const NOT_FOUND: &str = "not_found";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const AUTHENTICATION: &str = "authentication";
    pub const CONFLICT: &str = "conflict";
    pub const RATE_LIMITED: &str = "rate_limited";
    pub const INVALID_ARGUMENT: &str = "invalid_argument";
    pub const UNAVAILABLE: &str = "unavailable";
    pub const INTERNAL: &str = "internal";
    pub const DEADLINE_EXCEEDED: &str = "deadline_exceeded";
    pub const CANCELLED: &str = "cancelled";

    /// Every defined code, for validation and documentation.
    pub const ALL: &[&str] = &[
        NOT_FOUND,
        UNSUPPORTED,
        AUTHENTICATION,
        CONFLICT,
        RATE_LIMITED,
        INVALID_ARGUMENT,
        UNAVAILABLE,
        INTERNAL,
        DEADLINE_EXCEEDED,
        CANCELLED,
    ];
}

impl PluginError {
    /// An error with `code` and `message`.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        PluginError {
            code: code.to_string(),
            message: message.into(),
            ..Default::default()
        }
    }

    /// The method or capability is not implemented.
    pub fn unsupported(capability: impl Into<String>) -> Self {
        let capability = capability.into();
        PluginError {
            code: code::UNSUPPORTED.to_string(),
            message: format!("{capability} is not supported"),
            capability,
            ..Default::default()
        }
    }

    /// The plugin or its remote is temporarily unreachable; callers may retry.
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(code::UNAVAILABLE, message)
    }

    /// The request was malformed.
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(code::INVALID_ARGUMENT, message)
    }

    /// A plugin-side bug.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(code::INTERNAL, message)
    }

    /// The call was cancelled before it finished.
    pub fn cancelled() -> Self {
        Self::new(code::CANCELLED, "the call was cancelled")
    }

    /// The caller's deadline passed.
    pub fn deadline_exceeded(deadline_ms: u64) -> Self {
        Self::new(
            code::DEADLINE_EXCEEDED,
            format!("no answer within {deadline_ms} ms"),
        )
    }

    /// The remote throttled the plugin.
    pub fn rate_limited(retry_after_seconds: Option<u64>) -> Self {
        PluginError {
            code: code::RATE_LIMITED.to_string(),
            message: "rate limited".to_string(),
            retry_after_seconds,
            ..Default::default()
        }
    }

    /// Whether a caller may retry the same call later.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.code.as_str(),
            code::UNAVAILABLE | code::RATE_LIMITED | code::DEADLINE_EXCEEDED
        )
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PluginError {}
