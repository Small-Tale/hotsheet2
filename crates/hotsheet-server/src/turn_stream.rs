//! Bounded, stable client wire projection for raw AI-tool turn events (HS2-060HQJ).

use std::collections::BTreeMap;

use hotsheet_aitools::{DoneReason, TurnEvent};
use serde::Serialize;

const MAX_ORDINARY_EVENTS: usize = 128;
const MAX_LATE_CRITICAL_PER_KIND: usize = 8;
const MAX_OUTPUT_CHARS: usize = 65_536;
const MAX_NATIVE_PAYLOAD_BYTES: usize = 65_536;
const MAX_DIAGNOSTIC_CHARS: usize = 65_536;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TurnStreamEnvelope {
    pub connection_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    pub event: ClientTurnEvent,
}

/// Version-tolerant tagged turn event. New variants are additive; clients must ignore event
/// types they do not understand instead of terminating their live loop.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientTurnEvent {
    Output {
        content: String,
        truncated: bool,
    },
    PermissionAsked {
        tool: String,
        summary: String,
    },
    Usage {
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        tokens_in: u64,
        tokens_out: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
    },
    NativeActivity {
        source: String,
        payload: serde_json::Value,
    },
    Coalesced {
        total: usize,
        kinds: BTreeMap<String, usize>,
    },
    Done {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

#[derive(Default)]
pub struct TurnStreamGuard {
    ordinary: usize,
    late_critical: BTreeMap<String, usize>,
    dropped: BTreeMap<String, usize>,
}

impl TurnStreamGuard {
    /// Admit one raw event. `Done` flushes one exact dropped-count summary before itself.
    pub fn observe(&mut self, event: &TurnEvent) -> Vec<ClientTurnEvent> {
        if matches!(event, TurnEvent::Done(_)) {
            let mut output = self.take_summary();
            output.push(project(event));
            return output;
        }

        let kind = kind_name(event);
        if self.ordinary < MAX_ORDINARY_EVENTS {
            self.ordinary += 1;
            return vec![project(event)];
        }
        if matches!(event, TurnEvent::PermissionAsked(_) | TurnEvent::Usage(_)) {
            let count = self.late_critical.entry(kind.into()).or_default();
            if *count < MAX_LATE_CRITICAL_PER_KIND {
                *count += 1;
                return vec![project(event)];
            }
        }
        *self.dropped.entry(kind.into()).or_default() += 1;
        Vec::new()
    }

    pub fn transport_failed(&mut self, message: &str) -> Vec<ClientTurnEvent> {
        let mut output = self.take_summary();
        output.push(ClientTurnEvent::Done {
            reason: "failed".into(),
            exit_code: None,
            message: bounded_diagnostic(message),
        });
        output
    }

    fn take_summary(&mut self) -> Vec<ClientTurnEvent> {
        if self.dropped.is_empty() {
            return Vec::new();
        }
        vec![ClientTurnEvent::Coalesced {
            total: self.dropped.values().sum(),
            kinds: std::mem::take(&mut self.dropped),
        }]
    }
}

fn bounded_diagnostic(message: &str) -> Option<String> {
    let message = message.trim();
    if message.is_empty() {
        return None;
    }
    let mut chars = message.chars();
    let mut bounded = chars
        .by_ref()
        .take(MAX_DIAGNOSTIC_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        bounded.pop();
        bounded.push('…');
    }
    Some(bounded)
}

fn project(event: &TurnEvent) -> ClientTurnEvent {
    match event {
        TurnEvent::Output(content) => {
            let shortened = content.chars().take(MAX_OUTPUT_CHARS).collect::<String>();
            ClientTurnEvent::Output {
                truncated: shortened.len() != content.len(),
                content: shortened,
            }
        }
        TurnEvent::PermissionAsked(request) => ClientTurnEvent::PermissionAsked {
            tool: request.tool.clone(),
            summary: request.summary.clone(),
        },
        TurnEvent::Usage(usage) => ClientTurnEvent::Usage {
            model: usage.model.clone(),
            tokens_in: usage.tokens_in,
            tokens_out: usage.tokens_out,
            cost_usd: usage.cost_usd,
        },
        TurnEvent::NativeActivity { source, payload } => {
            let encoded_bytes = serde_json::to_vec(payload).map_or(0, |bytes| bytes.len());
            ClientTurnEvent::NativeActivity {
                source: source.clone(),
                payload: if encoded_bytes > MAX_NATIVE_PAYLOAD_BYTES {
                    serde_json::json!({
                        "truncated": true,
                        "original_bytes": encoded_bytes,
                    })
                } else {
                    payload.clone()
                },
            }
        }
        TurnEvent::Done(reason) => {
            let (reason, exit_code, message) = match reason {
                DoneReason::Completed => ("completed", None, None),
                DoneReason::Interrupted => ("interrupted", None, None),
                DoneReason::Failed { exit_code, message } => (
                    "failed",
                    Some(*exit_code),
                    message.as_deref().and_then(bounded_diagnostic),
                ),
            };
            ClientTurnEvent::Done {
                reason: reason.into(),
                exit_code,
                message,
            }
        }
    }
}

fn kind_name(event: &TurnEvent) -> &'static str {
    match event {
        TurnEvent::Output(_) => "output",
        TurnEvent::PermissionAsked(_) => "permission_asked",
        TurnEvent::Usage(_) => "usage",
        TurnEvent::NativeActivity { .. } => "native_activity",
        TurnEvent::Done(_) => "done",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_unicode_diagnostics_are_bounded_for_provider_and_transport_failures() {
        let diagnostic = "界".repeat(MAX_DIAGNOSTIC_CHARS + 20);
        for event in [
            project(&TurnEvent::Done(DoneReason::failed(1, diagnostic.clone()))),
            TurnStreamGuard::default()
                .transport_failed(&diagnostic)
                .remove(0),
        ] {
            let ClientTurnEvent::Done {
                message: Some(message),
                ..
            } = event
            else {
                panic!("expected diagnostic");
            };
            assert_eq!(message.chars().count(), MAX_DIAGNOSTIC_CHARS);
            assert!(message.ends_with('…'));
        }
        assert_eq!(bounded_diagnostic("  "), None);
        assert_eq!(bounded_diagnostic("短"), Some("短".into()));
    }

    #[test]
    fn failed_done_carries_provider_text_and_nonfailures_omit_it() {
        let failed = project(&TurnEvent::Done(DoneReason::failed(
            1,
            " Model at capacity ",
        )));
        assert_eq!(
            serde_json::to_value(failed).unwrap(),
            serde_json::json!({"type":"done","reason":"failed","exit_code":1,"message":"Model at capacity"})
        );
        for reason in [DoneReason::Completed, DoneReason::Interrupted] {
            assert!(
                serde_json::to_value(project(&TurnEvent::Done(reason)))
                    .unwrap()
                    .get("message")
                    .is_none()
            );
        }
    }

    #[test]
    fn bounds_noise_but_preserves_summary_usage_and_done() {
        let mut guard = TurnStreamGuard::default();
        for _ in 0..MAX_ORDINARY_EVENTS {
            assert_eq!(guard.observe(&TurnEvent::Output("x".into())).len(), 1);
        }
        assert!(
            guard
                .observe(&TurnEvent::Output("dropped".into()))
                .is_empty()
        );
        assert_eq!(
            guard
                .observe(&TurnEvent::Usage(hotsheet_aitools::Usage::default()))
                .len(),
            1
        );
        let end = guard.observe(&TurnEvent::Done(DoneReason::Completed));
        assert!(matches!(
            &end[..],
            [
                ClientTurnEvent::Coalesced { total: 1, .. },
                ClientTurnEvent::Done { reason, .. }
            ] if reason == "completed"
        ));
    }

    #[test]
    fn truncates_one_pathological_output_chunk() {
        let projected = project(&TurnEvent::Output("x".repeat(MAX_OUTPUT_CHARS + 1)));
        assert!(matches!(
            projected,
            ClientTurnEvent::Output { content, truncated: true }
                if content.len() == MAX_OUTPUT_CHARS
        ));
    }

    #[test]
    fn replaces_one_pathological_native_payload_with_a_bounded_marker() {
        let projected = project(&TurnEvent::NativeActivity {
            source: "test".into(),
            payload: serde_json::json!({"body": "x".repeat(MAX_NATIVE_PAYLOAD_BYTES)}),
        });
        assert!(matches!(
            projected,
            ClientTurnEvent::NativeActivity { payload, .. }
                if payload["truncated"] == true
                    && payload["original_bytes"].as_u64().unwrap() > MAX_NATIVE_PAYLOAD_BYTES as u64
        ));
    }

    #[test]
    fn a_transport_error_still_emits_terminal_done() {
        assert_eq!(
            TurnStreamGuard::default().transport_failed("transport unavailable"),
            [ClientTurnEvent::Done {
                reason: "failed".into(),
                exit_code: None,
                message: Some("transport unavailable".into()),
            }]
        );
    }
}
