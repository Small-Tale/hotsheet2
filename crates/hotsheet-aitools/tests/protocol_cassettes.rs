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
