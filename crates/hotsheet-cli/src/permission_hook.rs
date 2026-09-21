//! The native lifecycle-hook permission adapter (`docs/05` §5.7). Claude Code and Codex
//! invoke a configured command with documented hook JSON on stdin and read a decision on
//! stdout. Their `PermissionRequest` contracts intentionally share the same shape. This
//! module is the pure mapping between that contract and Hot Sheet's `(tool, action)` key;
//! the effectful part (read the running server's URL/secret from the env, POST
//! `/permissions/ask`, block for a human) lives in the `permission-hook` subcommand.

use serde_json::{Value, json};

/// A native lifecycle-hook permission decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookDecision {
    /// Auto-approve the tool use.
    Allow,
    /// Block it (Claude surfaces the reason).
    Deny,
    /// Defer to Claude's normal permission flow — used when Hot Sheet isn't governing this
    /// run (no server), so an always-installed hook never silently auto-approves.
    Ask,
}

/// Permission lifecycle event carried by the hook input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionHookEvent {
    /// Fires before every tool use. Hot Sheet consumes this only for marked headless runs.
    PreToolUse,
    /// Fires only when interactive Claude is about to present a native permission dialog.
    PermissionRequest,
    /// Any unrelated or malformed invocation; the adapter must leave it alone.
    Other,
}

/// Classify the lifecycle event so interactive prompts and headless tool calls can use
/// their distinct Claude response schemas.
pub fn permission_hook_event(input: &Value) -> PermissionHookEvent {
    match input.get("hook_event_name").and_then(Value::as_str) {
        Some("PreToolUse") => PermissionHookEvent::PreToolUse,
        Some("PermissionRequest") => PermissionHookEvent::PermissionRequest,
        _ => PermissionHookEvent::Other,
    }
}

/// Whether Hot Sheet should replace Claude's permission handling for this event.
pub fn should_bridge_permission(event: PermissionHookEvent, headless_pre_tool: bool) -> bool {
    event == PermissionHookEvent::PermissionRequest
        || event == PermissionHookEvent::PreToolUse && headless_pre_tool
}

/// Map a native hook **input** to the bridge's `(tool, action)` rule key. The
/// action is the command (Bash), else a file path (Edit/Write/Read), else empty — the same
/// coarse key codex uses, so an `Always` rule remembered on one transport matches the other.
pub fn hook_tool_action(input: &Value) -> (String, String) {
    let tool = input
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let tool_input = input.get("tool_input");
    let action = tool_input
        .and_then(|t| {
            t.get("command")
                .and_then(Value::as_str)
                .or_else(|| t.get("file_path").and_then(Value::as_str))
                .or_else(|| t.get("path").and_then(Value::as_str))
                .map(str::to_string)
        })
        .or_else(|| {
            tool_input
                .filter(|value| !value.is_null())
                .map(Value::to_string)
        })
        .unwrap_or_default();
    (tool, action)
}

/// The connection id the provider reports (its `session_id`), for route-back attribution.
pub fn hook_connection(input: &Value) -> String {
    input
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("interactive-hook")
        .to_string()
}

/// Render a [`HookDecision`] as the legacy/headless PreToolUse hook output.
pub fn hook_decision_json(decision: HookDecision) -> Value {
    let (word, reason) = match decision {
        HookDecision::Allow => ("allow", "approved via the Hot Sheet permission bridge"),
        HookDecision::Deny => ("deny", "denied via the Hot Sheet permission bridge"),
        HookDecision::Ask => ("ask", "Hot Sheet is not governing this run"),
    };
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": word,
            "permissionDecisionReason": reason,
        }
    })
}

/// Render a decision for the native interactive `PermissionRequest` hook shared by Claude
/// Code and Codex. Unlike `PreToolUse`, this event uses a nested permission-result object.
pub fn permission_request_decision_json(decision: HookDecision) -> Option<Value> {
    let decision = match decision {
        HookDecision::Allow => json!({"behavior":"allow"}),
        HookDecision::Deny => json!({
            "behavior":"deny",
            "message":"denied via the Hot Sheet permission bridge",
        }),
        HookDecision::Ask => return None,
    };
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": decision,
        }
    }))
}

/// Parse the server's `POST /permissions/ask` reply (`{"decision":"allow"|"deny"}`) into a
/// [`HookDecision`]. Anything unexpected is treated as `Deny` (the safe default).
pub fn decision_from_server(reply: &Value) -> HookDecision {
    match reply.get("decision").and_then(Value::as_str) {
        Some("allow") => HookDecision::Allow,
        _ => HookDecision::Deny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_bash_and_edit_inputs_to_tool_action() {
        let bash = json!({ "tool_name": "Bash", "tool_input": { "command": "rm -rf build" } });
        assert_eq!(
            hook_tool_action(&bash),
            ("Bash".into(), "rm -rf build".into())
        );
        let edit = json!({ "tool_name": "Edit", "tool_input": { "file_path": "src/main.rs" } });
        assert_eq!(
            hook_tool_action(&edit),
            ("Edit".into(), "src/main.rs".into())
        );
        // Missing pieces degrade to empty, never panic.
        assert_eq!(hook_tool_action(&json!({})), (String::new(), String::new()));

        // MCP/local-function arguments have no standard path field. Keep their complete
        // deterministic JSON payload so Always rules and repeated prompts still match.
        let mcp = json!({
            "tool_name": "mcp__github__create_issue",
            "tool_input": { "repo": "small-tale/hotsheet2", "title": "Retry me" }
        });
        assert_eq!(
            hook_tool_action(&mcp),
            (
                "mcp__github__create_issue".into(),
                r#"{"repo":"small-tale/hotsheet2","title":"Retry me"}"#.into()
            )
        );
    }

    #[test]
    fn distinguishes_real_permission_requests_from_pre_tool_observation() {
        assert_eq!(
            permission_hook_event(&json!({"hook_event_name":"PermissionRequest"})),
            PermissionHookEvent::PermissionRequest
        );
        assert_eq!(
            permission_hook_event(&json!({"hook_event_name":"PreToolUse"})),
            PermissionHookEvent::PreToolUse
        );
        assert_eq!(
            permission_hook_event(&json!({})),
            PermissionHookEvent::Other
        );
        assert!(should_bridge_permission(
            PermissionHookEvent::PermissionRequest,
            false
        ));
        assert!(!should_bridge_permission(
            PermissionHookEvent::PreToolUse,
            false
        ));
        assert!(should_bridge_permission(
            PermissionHookEvent::PreToolUse,
            true
        ));
    }

    #[test]
    fn connection_defaults_when_absent() {
        assert_eq!(hook_connection(&json!({ "session_id": "s-1" })), "s-1");
        assert_eq!(hook_connection(&json!({})), "interactive-hook");
    }

    #[test]
    fn decision_json_uses_claude_pretooluse_shape() {
        let v = hook_decision_json(HookDecision::Allow);
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(
            hook_decision_json(HookDecision::Deny)["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert_eq!(
            hook_decision_json(HookDecision::Ask)["hookSpecificOutput"]["permissionDecision"],
            "ask"
        );
    }

    #[test]
    fn permission_request_decision_uses_the_shared_native_result_shape() {
        let allow = permission_request_decision_json(HookDecision::Allow).unwrap();
        assert_eq!(
            allow["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
        assert_eq!(allow["hookSpecificOutput"]["decision"]["behavior"], "allow");
        let deny = permission_request_decision_json(HookDecision::Deny).unwrap();
        assert_eq!(deny["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert!(deny["hookSpecificOutput"]["decision"]["message"].is_string());
        assert_eq!(permission_request_decision_json(HookDecision::Ask), None);
    }

    #[test]
    fn server_reply_maps_to_decision() {
        assert_eq!(
            decision_from_server(&json!({ "decision": "allow" })),
            HookDecision::Allow
        );
        assert_eq!(
            decision_from_server(&json!({ "decision": "deny" })),
            HookDecision::Deny
        );
        // Garbage → deny (safe).
        assert_eq!(decision_from_server(&json!({})), HookDecision::Deny);
    }

    #[test]
    fn repeated_permission_requests_are_stateless_and_keep_native_fallbacks() {
        let input = json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "thread-1",
            "tool_name": "Bash",
            "tool_input": { "command": "git push" }
        });
        for reply in [
            json!({"decision":"allow"}),
            json!({"decision":"deny"}),
            json!({"decision":"allow"}),
        ] {
            let decision = decision_from_server(&reply);
            assert!(permission_request_decision_json(decision).is_some());
            assert_eq!(hook_tool_action(&input).1, "git push");
        }
        assert_eq!(
            permission_request_decision_json(HookDecision::Ask),
            None,
            "a hook transport failure emits no decision so the native prompt retries"
        );
    }
}
