//! HS2-86MK1Y: a spawn tool's **structured failure line** (`docs/26` §26.11).
//!
//! A one-shot tool such as Antigravity (`agy --print`) ends a failed turn with an exit
//! status and, since agy 1.5.10, one typed JSON line on stderr:
//! `AGY_ERROR: {"short_error":"…","status":"DEADLINE_EXCEEDED","retryable":false,…}`.
//! The plugin names the line's prefix in `[drive] error_line`. This module reads the
//! last such line's typed members only. Free text (the `short_error` member included) is
//! never searched for a limit, so this is not a second message parser.
//!
//! A limit is a `RESOURCE_EXHAUSTED` status, an HTTP 429 or a gRPC 8 code. A retryable
//! limit is the request-rate case (`rate_limit`); any other limit is a quota or usage cap
//! (`usage_limit_exceeded`). Both are error types `stuck_causes::is_usage_limit` accepts.

use serde_json::{Map, Value};

/// The error type of a retryable limit (a per-minute request rate or capacity limit).
pub const RATE_LIMIT: &str = "rate_limit";
/// The error type of a limit the tool does not retry (a daily, weekly or spend quota).
pub const USAGE_LIMIT: &str = "usage_limit_exceeded";

/// The longest message kept from a failure line, in characters.
const MAX_MESSAGE_CHARS: usize = 500;

/// What a structured failure line says about a failed turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredFailure {
    /// [`RATE_LIMIT`] or [`USAGE_LIMIT`] when the line reports a limit, else `None`.
    pub error_type: Option<&'static str>,
    /// The line's `short_error`, trimmed and bounded, for display only.
    pub message: Option<String>,
}

/// Members that carry an HTTP status code.
const HTTP_CODES: [&str; 3] = ["http_code", "http_status", "status_code"];
/// Members that carry a gRPC status code.
const GRPC_CODES: [&str; 2] = ["grpc_code", "grpc_status"];

/// The last `<prefix>{json}` line in `stderr`, read as a [`StructuredFailure`]. `None`
/// when there is no such line, the prefix is empty, or its payload is not a JSON object.
pub fn parse(stderr: &str, prefix: &str) -> Option<StructuredFailure> {
    if prefix.is_empty() {
        return None;
    }
    let object = stderr
        .lines()
        .rev()
        .filter_map(|line| line.trim_end_matches('\r').strip_prefix(prefix))
        .find_map(
            |payload| match serde_json::from_str::<Value>(payload.trim()) {
                Ok(Value::Object(object)) => Some(object),
                _ => None,
            },
        )?;
    let limit = is_limit(&object);
    let retryable = object.get("retryable").and_then(Value::as_bool) == Some(true);
    Some(StructuredFailure {
        error_type: limit.then_some(if retryable { RATE_LIMIT } else { USAGE_LIMIT }),
        message: object
            .get("short_error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty())
            .map(|message| message.chars().take(MAX_MESSAGE_CHARS).collect()),
    })
}

fn is_limit(object: &Map<String, Value>) -> bool {
    let code = |names: &[&str]| -> Vec<u64> {
        names
            .iter()
            .filter_map(|name| object.get(*name).and_then(Value::as_u64))
            .collect()
    };
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status.trim().eq_ignore_ascii_case("RESOURCE_EXHAUSTED"));
    status
        || code(&HTTP_CODES).contains(&429)
        || code(&GRPC_CODES).contains(&8)
        || code(&["code"]).iter().any(|code| matches!(code, 429 | 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "AGY_ERROR: ";

    fn read(stderr: &str) -> Option<StructuredFailure> {
        parse(stderr, PREFIX)
    }

    #[test]
    fn hs2_86mk1y_the_observed_print_timeout_line_is_a_failure_but_no_limit() {
        // The shape observed on agy 1.5.10 with a 1 ms --print-timeout (synthetic prompt).
        let stderr = "starting\nAGY_ERROR: {\"short_error\":\"The request timed out.\",\
            \"status\":\"DEADLINE_EXCEEDED\",\"retryable\":false,\"reason\":\"print_timeout\"}\n";
        assert_eq!(
            read(stderr),
            Some(StructuredFailure {
                error_type: None,
                message: Some("The request timed out.".into()),
            })
        );
    }

    #[test]
    fn hs2_86mk1y_limit_statuses_and_codes_map_by_retryability() {
        // Assumed quota-hit shapes (the real payload is unconfirmed, HS2-GETFE4).
        for (line, expected) in [
            (
                r#"{"status":"RESOURCE_EXHAUSTED","retryable":false}"#,
                USAGE_LIMIT,
            ),
            (
                r#"{"status":"resource_exhausted","retryable":true}"#,
                RATE_LIMIT,
            ),
            (r#"{"status":"UNKNOWN","http_code":429}"#, USAGE_LIMIT),
            (r#"{"http_status":429,"retryable":true}"#, RATE_LIMIT),
            (r#"{"status_code":429}"#, USAGE_LIMIT),
            (r#"{"grpc_code":8,"retryable":false}"#, USAGE_LIMIT),
            (r#"{"grpc_status":8}"#, USAGE_LIMIT),
            (r#"{"code":429,"retryable":true}"#, RATE_LIMIT),
            (r#"{"code":8}"#, USAGE_LIMIT),
        ] {
            let failure = read(&format!("{PREFIX}{line}")).unwrap();
            assert_eq!(failure.error_type, Some(expected), "{line}");
        }
        // Other codes and statuses are failures, not limits.
        for line in [
            r#"{"status":"UNAVAILABLE","code":503,"retryable":true}"#,
            r#"{"http_code":8,"grpc_code":429}"#,
            r#"{"status":"PERMISSION_DENIED","code":"429"}"#,
            r#"{"retryable":true}"#,
        ] {
            assert_eq!(
                read(&format!("{PREFIX}{line}")).unwrap().error_type,
                None,
                "{line}"
            );
        }
    }

    #[test]
    fn hs2_86mk1y_free_text_never_counts_as_a_limit() {
        let stderr = "Error 429 RESOURCE_EXHAUSTED: quota exceeded, try again later\n\
            AGY_ERROR: {\"short_error\":\"429 RESOURCE_EXHAUSTED quota exceeded\",\
            \"status\":\"INTERNAL\"}\n";
        let failure = read(stderr).unwrap();
        assert_eq!(failure.error_type, None);
        assert_eq!(
            failure.message.as_deref(),
            Some("429 RESOURCE_EXHAUSTED quota exceeded")
        );
        // Text alone, without the prefix, is no failure line at all.
        assert_eq!(read("429 RESOURCE_EXHAUSTED rate limit exceeded\n"), None);
    }

    #[test]
    fn hs2_86mk1y_prefix_and_json_shape_are_strict() {
        assert_eq!(
            parse("AGY_ERROR: {\"status\":\"RESOURCE_EXHAUSTED\"}", ""),
            None
        );
        for stderr in [
            "AGY_ERROR: not json",
            "AGY_ERROR: [\"RESOURCE_EXHAUSTED\"]",
            "AGY_ERROR: \"RESOURCE_EXHAUSTED\"",
            "  AGY_ERROR: {\"status\":\"RESOURCE_EXHAUSTED\"}",
            "agy_error: {\"status\":\"RESOURCE_EXHAUSTED\"}",
        ] {
            assert_eq!(read(stderr), None, "{stderr}");
        }
        // A CRLF line ending and surrounding space in the payload are accepted.
        let failure = read("AGY_ERROR:  {\"status\":\"RESOURCE_EXHAUSTED\"} \r\n").unwrap();
        assert_eq!(failure.error_type, Some(USAGE_LIMIT));
        assert_eq!(failure.message, None);
        // The message is trimmed, bounded and dropped when blank.
        let long = "x".repeat(2_000);
        let failure = read(&format!("{PREFIX}{{\"short_error\":\" {long} \"}}")).unwrap();
        assert_eq!(failure.message.unwrap().chars().count(), MAX_MESSAGE_CHARS);
        assert_eq!(
            read(&format!("{PREFIX}{{\"short_error\":\"  \"}}"))
                .unwrap()
                .message,
            None
        );
    }

    #[test]
    fn hs2_86mk1y_the_last_valid_line_wins() {
        let stderr = format!(
            "{PREFIX}{{\"status\":\"RESOURCE_EXHAUSTED\"}}\nnoise\n\
             {PREFIX}{{\"status\":\"INTERNAL\",\"short_error\":\"later\"}}\n{PREFIX}garbage\n"
        );
        assert_eq!(
            read(&stderr),
            Some(StructuredFailure {
                error_type: None,
                message: Some("later".into()),
            })
        );
    }
}
