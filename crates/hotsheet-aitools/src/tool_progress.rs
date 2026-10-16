//! Provider-neutral tool start/finish classification for a running turn (HS2-XV1VCR).
//!
//! The host tracks which tool call is in flight so a client can tell a long silent tool from a
//! hung turn. Each transport reports tool lifecycles in its own native activity vocabulary; this
//! module is the one place that reads them back into a start or a finish.

use serde_json::Value;

use crate::drive::TurnEvent;

/// Native activity source of a Claude stream/hook record.
pub const CLAUDE_HOOKS_SOURCE: &str = "claude-hooks";

/// One tool lifecycle edge read from a turn event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolTransition {
    /// A tool call began. `id` is the provider's call id when it reports one.
    Started { id: Option<String>, name: String },
    /// A tool call ended, successfully or not.
    Finished {
        id: Option<String>,
        name: Option<String>,
    },
}

/// Classify a turn event as a tool start or finish. Events that are not tool lifecycle edges,
/// and sub-agent records (Claude `parent_tool_use_id`), return `None`: a sub-agent's own tools
/// run inside the parent call, which stays the in-flight tool until it finishes.
pub fn tool_transition(event: &TurnEvent) -> Option<ToolTransition> {
    let TurnEvent::NativeActivity { source, payload } = event else {
        return None;
    };
    match source.as_str() {
        CLAUDE_HOOKS_SOURCE => claude_transition(payload),
        _ => None,
    }
}

fn claude_transition(payload: &Value) -> Option<ToolTransition> {
    if payload
        .get("parent_tool_use_id")
        .is_some_and(|parent| !parent.is_null())
    {
        return None;
    }
    let id = text(payload, "tool_use_id");
    let name = text(payload, "tool_name");
    match payload.get("hook_event_name").and_then(Value::as_str)? {
        "PreToolUse" => Some(ToolTransition::Started { id, name: name? }),
        "PostToolUse" | "PostToolUseFailure" => Some(ToolTransition::Finished { id, name }),
        _ => None,
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn native(source: &str, payload: Value) -> TurnEvent {
        TurnEvent::NativeActivity {
            source: source.into(),
            payload,
        }
    }

    #[test]
    fn claude_pre_and_post_tool_use_are_a_start_and_a_finish() {
        assert_eq!(
            tool_transition(&native(
                CLAUDE_HOOKS_SOURCE,
                json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_use_id": "t1"}),
            )),
            Some(ToolTransition::Started {
                id: Some("t1".into()),
                name: "Bash".into()
            })
        );
        for hook in ["PostToolUse", "PostToolUseFailure"] {
            assert_eq!(
                tool_transition(&native(
                    CLAUDE_HOOKS_SOURCE,
                    json!({"hook_event_name": hook, "tool_use_id": "t1"}),
                )),
                Some(ToolTransition::Finished {
                    id: Some("t1".into()),
                    name: None
                })
            );
        }
    }

    #[test]
    fn claude_sub_agent_records_and_nameless_starts_are_not_transitions() {
        assert_eq!(
            tool_transition(&native(
                CLAUDE_HOOKS_SOURCE,
                json!({"hook_event_name": "PreToolUse", "tool_name": "Read", "parent_tool_use_id": "task"}),
            )),
            None
        );
        assert_eq!(
            tool_transition(&native(
                CLAUDE_HOOKS_SOURCE,
                json!({"hook_event_name": "PreToolUse", "tool_name": " "}),
            )),
            None
        );
        assert_eq!(
            tool_transition(&native(
                CLAUDE_HOOKS_SOURCE,
                json!({"hook_event_name": "Stop"})
            )),
            None
        );
        assert_eq!(tool_transition(&TurnEvent::Output("hi".into())), None);
        assert_eq!(tool_transition(&native("other", json!({}))), None);
    }
}
