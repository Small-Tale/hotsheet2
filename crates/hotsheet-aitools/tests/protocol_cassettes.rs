//! Fast, offline replay of sanitized real-tool protocol contracts (docs/12 §12.7.7).

#[test]
fn recorded_codex_0148_token_usage_matches_the_production_parser() {
    let message: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/codex-0.148-token-usage.json")).unwrap();
    let usage = hotsheet_aitools::codex_notification_usage(&message).unwrap();
    assert_eq!(
        usage.tokens_in, 1314,
        "input already includes cached tokens"
    );
    assert_eq!(usage.tokens_out, 42);
    assert_eq!(usage.model, None);
    assert_eq!(usage.cost_usd, None);
}

#[test]
fn recorded_claude_21241_result_matches_the_production_parser() {
    let message: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/claude-2.1.241-result.json")).unwrap();
    let usage = hotsheet_aitools::claude_result_usage(&message).unwrap();
    assert_eq!(usage.tokens_in, 960, "base + cache read + cache creation");
    assert_eq!(usage.tokens_out, 55);
    assert_eq!(usage.model, None);
    assert_eq!(usage.cost_usd, Some(0.0125));
}

#[test]
fn opencode_acp_v1_contract_matches_the_live_client() {
    let messages: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/opencode-acp-v1-contract.json")).unwrap();
    hotsheet_aitools::validate_opencode_transcript(&messages).unwrap();
    let usage = hotsheet_aitools::acp_usage(messages.last().unwrap()).unwrap();
    assert_eq!((usage.tokens_in, usage.tokens_out), (4, 1));
}

#[test]
fn recorded_codex_01521_items_match_the_activity_mapper() {
    use hotsheet_ticketing::ActivityKind;
    let messages: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/codex-0.152.1-activity.json")).unwrap();
    let events = messages
        .iter()
        .map(|message| {
            hotsheet_ticketing::activity::codex_activity(
                &message["params"]["item"],
                "event-redacted",
                "time-redacted",
            )
            .expect("pinned item remains narratable")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            ActivityKind::Command,
            ActivityKind::Edit,
            ActivityKind::Plan,
            ActivityKind::ToolCall,
        ]
    );
    assert_eq!(events[0].detail["command"], "cargo test");
    assert_eq!(events[1].detail["path"], "src/main.rs");
    assert_eq!(events[3].detail["name"], "example.lookup");
}

#[test]
fn recorded_claude_21258_hooks_match_the_activity_mapper() {
    use hotsheet_ticketing::ActivityKind;
    let hooks: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/claude-2.1.258-hook-activity.json")).unwrap();
    let events = hooks
        .iter()
        .map(|hook| {
            hotsheet_ticketing::activity::claude_activity(hook, "event-redacted", "time-redacted")
                .expect("pinned hook remains narratable")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            ActivityKind::Command,
            ActivityKind::Edit,
            ActivityKind::ToolCall,
        ]
    );
    assert!(
        events
            .iter()
            .all(|event| event.session.as_deref() == Some("session-redacted"))
    );
}

/// HS2-RGMJSP: a *synthetic* stream-json transcript built from Claude Code's documented
/// stream-json shapes (not a live capture): a Task sub-agent whose records carry
/// `parent_tool_use_id`, a `run_in_background` Bash call, tool results, and background-task
/// `system` notifications. Only main-agent text is the reply; tool results finish their calls;
/// sub-agent work is labelled narration.
#[test]
fn synthetic_claude_subagent_stream_maps_results_and_attributes_sub_agent_work() {
    use hotsheet_aitools::TurnEvent;
    use hotsheet_aitools::tool_progress::{ToolTransition, tool_transition};
    let records: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/claude-stream-subagent-synthetic.json"
    ))
    .unwrap();
    let events = hotsheet_aitools::claude_stream_events(&records);

    let outputs = events
        .iter()
        .filter_map(|event| match event {
            TurnEvent::Output(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        outputs,
        [
            "I'll research this with a sub-agent and run the suite in the background.",
            "Here is the summary."
        ],
        "sub-agent text never joins the main reply"
    );

    let natives = events
        .iter()
        .filter_map(|event| match event {
            TurnEvent::NativeActivity { source, payload } => {
                assert_eq!(source, "claude-hooks");
                assert_eq!(payload["session_id"], "session-redacted");
                Some(payload)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let shape = natives
        .iter()
        .map(|payload| {
            (
                payload["hook_event_name"].as_str().unwrap(),
                payload["tool_name"].as_str().unwrap_or(""),
                payload["parent_tool_use_id"].as_str().unwrap_or(""),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        shape,
        [
            ("PreToolUse", "Task", ""),
            ("PreToolUse", "Bash", ""),
            ("PostToolUse", "Bash", ""),
            ("TaskNotification", "", ""),
            ("SubagentMessage", "", "toolu_task_1"),
            ("PreToolUse", "WebFetch", "toolu_task_1"),
            ("PostToolUseFailure", "WebFetch", "toolu_task_1"),
            ("SubagentMessage", "", "toolu_task_1"),
            ("PostToolUse", "Task", ""),
            ("TaskNotification", "", ""),
        ]
    );
    assert_eq!(natives[4]["text"], "Searching the reference docs.");
    assert_eq!(natives[9]["status"], "completed");
    assert_eq!(natives[9]["summary"], "cargo test passed");
    assert!(matches!(events.last(), Some(TurnEvent::Done(_))));

    // In-flight tracking: the sub-agent's own calls never open; its parent Task stays open
    // until its result; the backgrounded command closes on its immediate result.
    let transitions = events
        .iter()
        .filter_map(tool_transition)
        .collect::<Vec<_>>();
    assert_eq!(
        transitions,
        [
            ToolTransition::Started {
                id: Some("toolu_task_1".into()),
                name: "Task".into()
            },
            ToolTransition::Started {
                id: Some("toolu_bg_1".into()),
                name: "Bash".into()
            },
            ToolTransition::Finished {
                id: Some("toolu_bg_1".into()),
                name: Some("Bash".into())
            },
            ToolTransition::Finished {
                id: Some("toolu_task_1".into()),
                name: Some("Task".into())
            },
        ]
    );

    // The activity timeline labels sub-agent work and narrates background tasks.
    let summaries = natives
        .iter()
        .filter_map(|payload| {
            hotsheet_ticketing::activity::claude_activity(
                payload,
                "event-redacted",
                "time-redacted",
            )
            .map(|event| event.summary)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summaries,
        [
            "claude called Task",
            "claude ran `cargo test`",
            "claude started a background task: cargo test",
            "claude sub-agent: Searching the reference docs.",
            "claude called WebFetch (sub-agent)",
            "claude sub-agent: Found the answer in the reference.",
            "claude background task completed: cargo test passed",
        ]
    );
}

#[test]
fn recorded_codex_01621_session_queue_matches_the_push_adapter() {
    // HS2-FKM97G: the SessionStart hook's `session_id` is the thread id `codex queue` takes.
    use hotsheet_aitools::session_push::{codex_error_line, queued_message_id};
    let cassette: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/codex-0.162.1-session-queue.json")).unwrap();
    let thread = cassette["session_start_hook_stdin"]["session_id"]
        .as_str()
        .unwrap();
    assert_eq!(cassette["queue_argv"][3], thread);
    // A queued message arrives as an ordinary user turn, so UserPromptSubmit clears a halt.
    assert_eq!(
        cassette["queued_user_prompt_submit_hook_stdin"]["hook_event_name"],
        "UserPromptSubmit"
    );
    assert_eq!(
        queued_message_id(cassette["queue_success"]["stdout"].as_str().unwrap()).as_deref(),
        Some("00000000-0000-7000-8000-000000000002")
    );
    let rejected = &cassette["queue_unknown_thread"];
    let reason = codex_error_line(
        rejected["stderr"].as_str().unwrap(),
        rejected["stdout"].as_str().unwrap(),
    );
    assert!(
        reason.starts_with("failed to queue session message"),
        "{reason}"
    );
    assert!(
        reason.contains("no rollout found for thread id"),
        "{reason}"
    );
}
