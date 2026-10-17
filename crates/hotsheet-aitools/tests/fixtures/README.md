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

`opencode-1.17.18-session-push.json` (HS2-PEJ962) is a sanitized capture of OpenCode's own
HTTP server behind a TUI launched with `--port` and `OPENCODE_SERVER_PASSWORD`: session list /
create / TUI select, toast, `prompt_async`, TUI `execute-command`, the unknown-session `404`, and
the wrong-password `401`. `tests/opencode_session_push.rs` replays it through the production
session-push adapter and `ureq` port against a loopback fake server. The directory is a
placeholder; the password and machine paths are removed.

`claude-stream-subagent-synthetic.json` (HS2-RGMJSP) is **synthetic, not a live capture**: it
is assembled from Claude Code's documented stream-json record shapes to pin the channel's
mapping of `tool_result` echoes, `parent_tool_use_id` sub-agent records, a
`run_in_background` Bash call, and background-task `system` notifications. Replace it with a
sanitized live capture (and note any shape differences) when the creds-gated capture runs.

HS2-MN865E adds `codex-0.162.1-thread-rejoin-failure.json`: what a read-only `thread/resume` rejoin of a
Codex TUI thread received when the TUI turn failed on an invalid model (`turn/started`, `error
{willRetry: false}`, `turn/completed` failed). Ids and paths are synthetic; the resume response is truncated.
