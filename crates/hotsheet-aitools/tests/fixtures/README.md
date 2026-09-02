# Protocol cassettes

Sanitized protocol-contract messages captured by the deliberate live verification in
HS2-CQ6B96 on 2026-08-24. The filenames pin the producing tool version. They contain no
prompts, credentials, user paths, or real session identifiers.

Fast CI replays these messages through the production usage parsers. Updating a fixture
requires a deliberate live run and a note explaining the observed protocol change; live
tests remain ignored and credentials-gated.

The OpenCode cassette pins ACP v1 initialize/session/update/prompt landmarks; its opt-in
live test exercises the same contract against `opencode acp`.

HS2-SW655F adds sanitized activity cassettes for Codex 0.152.1 and Claude Code
2.1.258. The Codex cassette pins the generated `item/completed` schema and its
`commandExecution`, `fileChange`, `plan`, and `mcpToolCall` item vocabulary. The Claude
cassette pins the `PreToolUse` hook stdin contract used by the installed permission hook;
the channel projects authoritative assistant `tool_use` blocks into that same contract.
Paths, ids, tool inputs, output, and diffs are synthetic or redacted. Credentialed drift
checks remain the ignored `HOTSHEET_CODEX_LIVE=1` and `HOTSHEET_CLAUDE_LIVE=1` tests;
ordinary gates never launch a model.
