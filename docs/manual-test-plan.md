# Manual Test Plan

Only behavior that cannot yet be exercised reliably in automated CI belongs here.
When automation lands, remove the manual-only step and record it below.

## Current manual checks

### OS credential stores

1. On macOS, pipe a disposable value into `hotsheet-cli key set test-provider` and
   confirm Keychain Access shows a `com.smalltale.hotsheet2` generic password.
2. Verify `key get`, `key list`, and `key delete`, and confirm neither
   `${HOTSHEET_HOME}/keys.json` nor any settings file contains the value.
3. Repeat on Linux with a live Secret Service session. An unavailable service must fail
   closed; `HOTSHEET_API_KEY_TEST_PROVIDER` is the explicit read-only fallback.

### Cross-device terminal sizing

1. Attach macOS and iOS-sized viewers to the same remote terminal.
2. Move focus between viewers and verify the configured sizing policy, letterboxing,
   and lease expiry without oscillation.
3. Disconnect the focused viewer and verify the remaining viewer takes ownership.

### Real-device mTLS enrollment

1. Enroll a second physical device against an off-loopback server.
2. Verify read-only and read-write ACLs independently.
3. Revoke and renew certificates and confirm enforcement without restarting the server.

### Native client UX

Exercise platform accessibility, background/resume, notification presentation, and
credential storage once the native clients exist.

### OpenCode ACP live compatibility

Run `HOTSHEET_OPENCODE_LIVE=1 cargo test -p hotsheet-aitools
opencode_live_acp_turn -- --ignored --nocapture` with a configured OpenCode provider.
This verifies initialization, session creation, a streamed prompt, and completion against
the installed executable. Last verified successfully on 2026-08-25 with OpenCode 1.17.18.

## Automated Coverage Summary

- Terminal sizing policy transitions and disconnect healing are automated in Rust and
  server WebSocket tests; only the real multi-device presentation remains manual.
- mTLS certificate/ACL behavior is automated; only physical-device enrollment UX remains.
- ACP wire parsing has scripted unit and contract-fixture coverage; the provider-backed
  OpenCode smoke above guards executable/provider integration drift.
