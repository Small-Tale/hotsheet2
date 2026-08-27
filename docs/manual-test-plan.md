# Manual Test Plan

Only behavior that cannot yet be exercised reliably in automated CI belongs here.
When automation lands, remove the manual-only step and record it below.

## Current manual checks

### OS credential stores

1. On macOS, run `hotsheet-cli key set test-provider`, confirm it displays one hidden
   prompt (typed characters are not echoed), and confirm Keychain Access shows a
   `com.smalltale.hotsheet2` generic password. Repeat with a piped disposable value.
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

### Web visual quality gate

For every change affecting rendered web-client visuals, inspect the real affected
`/ux-demo` components in a browser before completing the ticket:

1. Exercise every changed state and transition, including closed/open, hover, focus,
   selected, empty, populated, and disabled states that apply.
2. Inspect at a wide desktop viewport and at the narrowest supported component or
   workspace width; also inspect any breakpoint directly affected by the change.
3. Critique correctness and aesthetics: clipping, overflow, alignment, spacing,
   typography, contrast, icon visibility/centering, responsive flow, consistency with
   neighboring controls, and conformance with the wireframes and platform conventions.
4. Use screenshots for side-by-side comparison when useful. Record the inspected
   routes, states, and viewport sizes on the Hot Sheet ticket.
5. Keep automated interaction and geometry assertions as regression coverage, but do
   not treat them as proof of visual appeal or final conformance.

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
