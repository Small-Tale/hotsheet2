//! The **Claude Code PreToolUse hook** adapter (`docs/05` §5.7, HS2-YMR9HE) — the second
//! permission transport (the codex approval path is HS2-Q1F6HV). Claude invokes a
//! configured hook command before each tool use, passing the tool + input on stdin and
//! reading a decision on stdout. This module is the pure mapping between Claude's hook JSON
//! and the Hot Sheet permission bridge's `(tool, action)` key + allow/deny/ask decision;
//! the effectful part (read the running server's URL/secret from the env, POST
//! `/permissions/ask`, block for a human) lives in the `permission-hook` subcommand.

use serde_json::{Value, json};

/// A PreToolUse permission decision Claude understands.
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

/// Map a Claude PreToolUse hook **input** to the bridge's `(tool, action)` rule key. The
/// action is the command (Bash), else a file path (Edit/Write/Read), else empty — the same
/// coarse key codex uses, so an `Always` rule remembered on one transport matches the other.
pub fn hook_tool_action(input: &Value) -> (String, String) {
    let tool = input
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let action = input
        .get("tool_input")
        .and_then(|t| {
            t.get("command")
                .and_then(Value::as_str)
                .or_else(|| t.get("file_path").and_then(Value::as_str))
                .or_else(|| t.get("path").and_then(Value::as_str))
                .map(str::to_string)
        })
        .unwrap_or_default();
    (tool, action)
}

/// The connection id Claude reports (its `session_id`), for route-back attribution.
pub fn hook_connection(input: &Value) -> String {
    input
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("claude")
        .to_string()
}

/// Render a [`HookDecision`] as the PreToolUse hook **output** JSON Claude reads.
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
    }

    #[test]
    fn connection_defaults_when_absent() {
        assert_eq!(hook_connection(&json!({ "session_id": "s-1" })), "s-1");
        assert_eq!(hook_connection(&json!({})), "claude");
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
}
