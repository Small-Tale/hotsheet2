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
