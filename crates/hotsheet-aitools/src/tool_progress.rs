//! Provider-neutral tool start/finish classification for a running turn (HS2-XV1VCR).
//!
//! The host tracks which tool call is in flight so a client can tell a long silent tool from a
//! hung turn. Each transport reports tool lifecycles in its own native activity vocabulary; this
//! module is the one place that reads them back into a start or a finish.

use serde_json::Value;

use crate::drive::TurnEvent;

/// Native activity source of a Claude stream/hook record.
pub const CLAUDE_HOOKS_SOURCE: &str = "claude-hooks";
/// Native activity source of a completed Codex transcript item.
pub const CODEX_TRANSCRIPT_SOURCE: &str = "codex-transcript";
/// Native activity source of a Codex item that has started but not completed (HS2-90FMR2).
pub const CODEX_ITEM_STARTED_SOURCE: &str = "codex-item-started";

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
        CODEX_ITEM_STARTED_SOURCE => {
            let name = codex_tool_name(payload)?;
            Some(ToolTransition::Started {
                id: text(payload, "id"),
                name,
            })
        }
        CODEX_TRANSCRIPT_SOURCE => {
            let name = codex_tool_name(payload)?;
            Some(ToolTransition::Finished {
                id: text(payload, "id"),
                name: Some(name),
            })
        }
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

/// The display name of a Codex tool-like transcript item, or `None` for message items.
pub(crate) fn codex_tool_name(item: &Value) -> Option<String> {
    let kind = item.get("type").and_then(Value::as_str)?;
    Some(match kind {
        "commandExecution" => "Shell".to_owned(),
        "mcpToolCall" => text(item, "tool").unwrap_or_else(|| "MCP tool".to_owned()),
        "dynamicToolCall" => text(item, "tool").unwrap_or_else(|| "Tool".to_owned()),
        "webSearch" => "Web search".to_owned(),
        "fileChange" => "File edit".to_owned(),
        "collabAgentToolCall" => "Agent".to_owned(),
        "imageGeneration" => "Image generation".to_owned(),
        _ => return None,
    })
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

    #[test]
    fn codex_items_start_and_finish_by_item_id() {
        let item = json!({"type": "commandExecution", "id": "item-1", "command": "ls"});
        assert_eq!(
            tool_transition(&native(CODEX_ITEM_STARTED_SOURCE, item.clone())),
            Some(ToolTransition::Started {
                id: Some("item-1".into()),
                name: "Shell".into()
            })
        );
        assert_eq!(
            tool_transition(&native(CODEX_TRANSCRIPT_SOURCE, item)),
            Some(ToolTransition::Finished {
                id: Some("item-1".into()),
                name: Some("Shell".into())
            })
        );
        for (item, name) in [
            (
                json!({"type": "mcpToolCall", "tool": "hotsheet_get"}),
                "hotsheet_get",
            ),
            (json!({"type": "mcpToolCall"}), "MCP tool"),
            (json!({"type": "dynamicToolCall"}), "Tool"),
            (json!({"type": "dynamicToolCall", "tool": "x"}), "x"),
            (json!({"type": "webSearch"}), "Web search"),
            (json!({"type": "fileChange"}), "File edit"),
            (json!({"type": "collabAgentToolCall"}), "Agent"),
            (json!({"type": "imageGeneration"}), "Image generation"),
        ] {
            assert_eq!(
                tool_transition(&native(CODEX_ITEM_STARTED_SOURCE, item)),
                Some(ToolTransition::Started {
                    id: None,
                    name: name.into()
                })
            );
        }
        for item in [
            json!({"type": "agentMessage"}),
            json!({"type": "reasoning"}),
            json!({}),
        ] {
            assert_eq!(
                tool_transition(&native(CODEX_ITEM_STARTED_SOURCE, item)),
                None
            );
        }
    }
}
