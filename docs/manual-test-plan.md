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
4. On Windows, repeat set/get/list/delete and confirm the generic credential appears in
   Credential Manager under `com.smalltale.hotsheet2/test-provider`. Confirm a missing
   credential is reported as absent and other Win32 failures remain visible.

### Cross-device terminal sizing

1. Attach macOS and iOS-sized viewers to the same remote terminal.
2. Move focus between viewers and verify the configured sizing policy, letterboxing,
   and lease expiry without oscillation.
3. Disconnect the focused viewer and verify the remaining viewer takes ownership.
4. HS2-3ZBQDG — with both devices attached and idle (only heartbeats), confirm the PTY
   size stays put and neither device shows the other's size (no ~5s oscillation, no
   "empty"/clipped terminal on the phone). Then interact on the phone (tap/type) and
   confirm the PTY follows the phone after the focus-hold; interact back on the desktop
   and confirm it follows the desktop. Interaction, not the heartbeat, transfers control.
5. HS2-Z84F78 — on a real phone, open the Workspace grid and magnify a terminal. Confirm it
   fills the screen (no 5:3 letterbox), shows a full 80 columns scaled to the phone width, and
   uses M rows to fill the height (many more than 24), and that a full-width TUI (e.g. `nano`,
   `htop`) renders without horizontal wrapping. Rotate the device and confirm M recomputes.
6. HS2-3ZBQDG — on current **physical-device Mobile Safari**, use the server's ordinary
   LAN HTTP address (not localhost or an HTTPS tunnel), then repeat on HTTPS. Confirm visible
   scrollback glyphs in the Workspace grid, magnified terminal, and dedicated drawer. Type a
   unique line and confirm its visible glyphs after attach, focus, background/resume, device
   rotation, and reconnect. A renderer label or nonzero viewport size is insufficient: inspect
   painted text. LAN HTTP must work with `crypto.randomUUID` absent and `getRandomValues`
   present. If initialization is deliberately blocked, verify a readable terminal-local alert
   replaces the black surface. Automated actual-WebKit insecure-origin/PTY pixel coverage
   complements this physical device check; it does not emulate iOS background suspension.
7. HS2-S708S3 — in that dedicated drawer terminal, confirm the PTY stays 80 columns, chooses
   M rows to fill the available drawer height, width-fits without clipping, and recomputes M
   after rotation or drawer-height changes. Confirm read-only grid previews remain 80×24.

### Browser identities on ordinary LAN HTTP

HS2-76ZR5P — on physical Mobile Safari at the server's LAN HTTP address, create a ticket
with multiple attachments, cancel another draft then create again, draw two annotations,
regroup attachments, and open linked ticket readers. Add, rename, remove and recreate a
workspace visibility group. Create two AI chats, send turns, and reopen a saved conversation
twice. Confirm both new objects remain independent, each attachment gesture shares its batch,
and error/retry flows remain usable. Repeat after reload and over HTTPS. Automated WebKit
coverage checks the real insecure-origin capability boundary; device suspension remains a
manual check.

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

`clients/web` includes `domotion-svg` as a development dependency. Its bundled
Chromium is the local screenshot-review fallback when an interactive browser is not
attached; Playwright remains appropriate for automated interaction assertions.

### Settings view persists across projects

`settingsCategory`/`setSettingsCategory` are module-internal to `clients/web/src/main.tsx`
with no isolated unit surface, and the behavior needs two open projects, so verify
manually: open two projects, enter Settings and select a non-default view (e.g. AI tools)
in one, switch to the other project, and confirm it shows the same Settings view rather
than resetting to Ticket sources (HS2-4J50K3).

### New ticket survives an eventually-consistent index (HS2-Y5PDHW)

The retention logic (`clients/web/src/pending-created-tickets.ts`) is unit-tested with
transition-matrix coverage, but the end-to-end race needs a real server whose ticket-list
index lags a moment behind the just-created file plus a concurrent background refresh, which
is not reliably reproducible in a mocked browser test. Verify manually against a running
server: create several tickets in quick succession (and while an AI worker is renewing
claims, so background poll refreshes fire), and confirm every created ticket stays visible
and selected in the list — it must never briefly vanish and reappear. The optimistic row is
retained until a fetched page actually contains it or ~30s elapse.

### OpenCode ACP live compatibility

Run `HOTSHEET_OPENCODE_LIVE=1 cargo test -p hotsheet-aitools
opencode_live_acp_turn -- --ignored --nocapture` with a configured OpenCode provider.
This verifies initialization, session creation, a streamed prompt, and completion against
the installed executable. Last verified successfully on 2026-08-25 with OpenCode 1.17.18.

### Codex interactive permission-hook compatibility

Run `HOTSHEET_CODEX_LIVE=1 cargo test -p hotsheet-cli
launch_codex_interactive_permission_contract -- --ignored --nocapture` with authenticated
Codex CLI credentials. The test installs a vetted temporary project hook, causes a real
Codex turn to request Bash approval, verifies the documented `PermissionRequest` payload,
returns a native deny, and confirms the command did not run. Production launches retain
Codex's `/hooks` hash-review gate; only this isolated drift test bypasses persisted trust.

## Automated Coverage Summary

- Terminal sizing policy transitions and disconnect healing are automated in Rust and
  server WebSocket tests; only the real multi-device presentation remains manual.
- mTLS certificate/ACL behavior is automated; only physical-device enrollment UX remains.
- ACP wire parsing has scripted unit and contract-fixture coverage; the provider-backed
  OpenCode smoke above guards executable/provider integration drift.
- Codex hook generation, allow/deny/fallback/retry mapping, timeout behavior, setup merge
  safety, and subdirectory launch are automated without credentials; the gated provider
  smoke above guards the installed Codex lifecycle contract.
