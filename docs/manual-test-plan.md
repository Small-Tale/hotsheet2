# Manual Test Plan

Only behavior that cannot yet be exercised reliably in automated CI belongs here.
When automation lands, remove the manual-only step and record it below.

## Current manual checks

### Gallery annotation conflict (HS2-8SD6XQ, HS2-KTHP6F)

With a real attachment provider, open the same image in two clients, save a markup
change in one, then finish a conflicting draft in the other. Confirm the latter
keeps its draft and reports the server conflict. Retry or discard, then confirm
the displayed ticket matches the server result. Inspect the failed-save banner
at wide and phone widths.

### Repository setup and paging (HS2-J4WR11)

- In a project folder without Git, open Repository Status at wide and narrow widths. Initialize, enter an unavailable remote, retry with a valid remote, and confirm existing files remain untracked.
- In a repository with more than one page of changed files or commits, scroll to load the next page. Switch projects while a page loads and check that the old result does not appear. Reset after an empty page and confirm new files appear.

### WebKit list scroll after ticket mutation (HS2-KTXBK2)

At wide and phone widths, scroll a long ticket list deep into its rows, change a
visible ticket's status, and confirm the same nearby rows remain readable without
a flash to the top. Review the before/after captures attached to HS2-KTXBK2 for
clipping, row alignment, and any unexpected jump. The browser regression asserts
the scroll offsets; this visual review checks the rendered transition.

### Native Codex terminal usage (HS2-J14225)

1. Launch a Codex terminal in Hot Sheet with the shared Codex daemon enabled. Claim one ticket from that terminal's session, run a turn with nonzero token use, and open that ticket's Timeline. Confirm the Codex model (when reported), tokens, and per-model total appear at wide and phone widths.
2. Release the claim, run another turn, and confirm that turn stays in project metrics without increasing the ticket total. Repeat with two simultaneous claims; neither ticket should receive the ambiguous turn.
3. Repeat with `--no-daemon` to confirm the timeline makes no unsupported usage claim. The protocol cassette covers the watcher, but CI cannot reliably exercise an installed interactive Codex TUI and its daemon together.

### Codex Work claim-release usage (HS2-2K2D7E)

1. In a local Codex Work session outside Hot Sheet's Drive and terminal, claim one
   disposable ticket with the session's `HOTSHEET_WORKER_ID`. Run a turn that reports
   nonzero usage, release the claim, then select the ticket in Hot Sheet. Confirm its
   Timeline shows the model and input/output total recorded during the claim.
2. Run another turn after release and confirm it is not added to that ticket. Repeat
   with two overlapping claims; ambiguous turns must not appear on either ticket.
3. Release the same ticket again and reload the timeline. Counts must not double.
   Inspect `metrics/raw/<day>.jsonl` and logs for prompt or tool content; only numeric
   counts, model, timestamp, session/response identity, and ticket ID may be stored.
4. If Codex changes its local rollout format, confirm capture skips unrecognized
   records without assigning usage to another ticket. Existing unattributed raw
   metrics must remain unattributed.

### Active Codex Work usage (HS2-NFQ1NK)

1. In a local Codex Work session bound to a ticket store and worker, claim a ticket and run a turn. Run `hotsheet-cli metrics --refresh-codex`, open the ticket's Timeline, and confirm numeric usage appears before releasing the claim. Run another turn, renew the claim, then reopen Timeline and confirm only the new response was added.
2. Release the claim, run another turn, and refresh metrics. Confirm the post-release response is not added. Restart Hot Sheet, reopen the Timeline, and confirm the count is unchanged. Repeat with overlapping live claims and confirm ambiguous responses remain unassigned.

### Full-history grid tile resource profile (HS2-QV814W)

On a representative desktop and physical phone, create more running terminals than the
configured warm-terminal limit. Fill each with several thousand history lines, then open
the project grid and Workspace Grid. Record browser process memory and CPU at idle and
while output is streaming, first with warm terminals set to zero and then with a small
nonzero cap (for example four). Include the browser, device, history size, tile count,
cap, and sampling interval. Confirm no more than the cap have full instances, the other
tiles remain readable previews, and scrolling or opening a capped tile in the drawer is
responsive. Repeat after lowering and raising the cap. CI asserts instance counts and
handoffs; its synthetic socket load and browser memory APIs cannot give dependable
cross-browser process memory or CPU measurements.

### Development latency monitor (HS2-ZGBQ5Q)

Automation runs with `navigator.webdriver`, where the monitor is off unless opted in, so
the default-on path is checked by hand.

1. Run `npm run dev` from `clients/web` and open the app in an ordinary browser (do not
   set `localStorage['hotsheet-dev-latency']`).
2. Throttle the network in DevTools (Slow 4G) and click a ticket row. Confirm a
   warning toast appears at the bottom start, the console prints a
   `[hotsheet dev latency]` line, and the dev-server terminal prints the report and
   names `~/.hotsheet2/logs/dev-latency.jsonl`, which gains a JSON line.
3. Set `localStorage['hotsheet-dev-latency'] = 'off'`, reload, and confirm no toast or
   report appears for the same click.

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

### Encrypted remembered permissions (HS2-1S4QEX)

1. On macOS Keychain, Windows Credential Manager, and Linux Secret Service, approve a
   disposable exact command with Always. Restart the server and confirm only that
   command in the same project is auto-answered. Check the OS credential entry exists.
2. Inspect the machine-local permission JSON and lock file: owner-only file access,
   version-4 envelope, no command, path, key, or safe synopsis in plaintext. Check
   exported files and logs for the same secret string.
3. With a disposable store, lock the credential service, remove its key, and corrupt
   the ciphertext in separate runs. Confirm no remembered action is auto-approved;
   an attempted Always answer remains pending and reports an unavailable store. Restore
   the service or explicitly clear the unusable rule file before fresh approval.
4. Present a pre-v3 fingerprint-only array and its `.action-key`, restart, and confirm
   the old answers do not activate, the file becomes an empty encrypted envelope, and
   the obsolete key file is removed.
5. Open several hosted stores with existing v3 permission files. Confirm each migrates
   to v4 without losing its approvals. On the next server launch, confirm all stores
   read the same master Keychain item once; an ad hoc signed rebuild may prompt once
   again for that item. After removing the master and restarting, every existing v4
   vault must be disabled.
6. With a live server, make the vault temporarily unavailable, then restore the same
   authenticated file and credential. Reload the Settings page without restarting the
   server: persistent answers return once, while a chat answer, session answer, and
   pending request remain. Repeat with a different authenticated vault file and confirm
   only its persistent answers return (HS2-8DAA0P).

### Legacy permission migration (HS2-ZQ6TV2)

1. With two registered projects sharing a Git ticket store, remember a disposable approval in the old store namespace. Open one project's Settings → Permissions → Legacy answers, review the source, and confirm that the exact action and decision are legible only in the review dialog.
2. Cancel once, then select the answer and copy it. Confirm it appears only in the chosen project's remembered answers, while the other project still has no project-scoped copy. Retry the same selection and confirm there is one copy.
3. Repeat from a terminal with `hotsheet-cli permission-migrate --project <checkout-id> --source <connection-id>` and then `--approve --id <candidate-id>`. Confirm the legacy vault remains readable. Remove or lock either vault and confirm the copy fails without granting the action.

### Permission secret suggestions (HS2-5ZRSJW)

1. Ask for approval of a disposable command such as `API_TOKEN=abc123 npm test --password=def456`.
   Start Allow for This Chat or Always and verify the review starts at Exact action.
   Use the token hint, inspect its placeholder, then remove it and verify exact matching
   returns. Use both hints, edit the expression, and confirm only after checking the scope.
2. Before confirmation, check that the request is still pending and no remembered rule
   exists. After confirmation, repeat with a changed disposable value and verify only
   the reviewed pattern is eligible to auto-approve.
3. Try quoted values, an Authorization bearer header, a URL credential and secret query
   key, and a `/secrets/` path. Check each offset highlights the intended text. Then try
   a multiline command, unmatched quote, shell pipe, dynamic expansion, and example
   values such as `demo`; these should offer no hint. Missing hints are possible: the
   classifier is intentionally conservative and is not a secret detector.
4. Inspect the popup at wide and phone widths. Verify that long actions and suggested
   spans wrap, the individual Use/Remove controls remain reachable, and no raw value
   appears in server diagnostics or exported rule metadata beyond the already reviewed
   encrypted exact action.

### Cross-device terminal sizing

1. Attach macOS and iOS-sized viewers to the same remote terminal.
2. Interact with each terminal in turn and verify the configured sizing policy and lease
   expiry without oscillation. Move keyboard focus to surrounding app controls and confirm
   each visible interactive terminal remains fitted with no focus-to-resize badge.
3. Disconnect the focused viewer and verify the remaining viewer takes ownership.
4. HS2-3ZBQDG — with both devices attached and idle (only heartbeats), confirm the PTY
   size stays put and neither device shows the other's size (no ~5s oscillation, no
   "empty"/clipped terminal on the phone). Then interact on the phone (tap/type) and
   confirm the PTY follows the phone after the focus-hold; interact back on the desktop
   and confirm it follows the desktop. Interaction, not the heartbeat, transfers control.
5. HS2-SA08XA — on a real phone, open the Workspace grid and magnify a terminal. Confirm it
   fills the screen (no 5:3 letterbox), keeps 80×24 scaled to the available frame, and that a
   full-width TUI (e.g. `nano`, `htop`) renders without horizontal wrapping. Rotate the device
   and confirm the scale recomputes without changing the grid.
   HS2-WMN626 — on a real iPhone (Safari and the installed PWA), magnify a terminal and tap into
   it. Confirm the terminal shrinks to sit above the virtual keyboard (nothing hidden behind it),
   the top toolbar hides while the keyboard is up and returns when it is dismissed, the status
   bar/home-indicator safe areas are respected, and dragging or typing never pans the terminal
   sideways. Confirm magnification has no column-changing text-size button, then close with the
   toolbar's X. Automated coverage simulates the keyboard through a
   stubbed `VisualViewport`; only a device proves iOS's real keyboard and focus-scroll behavior.
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
8. HS2-GMTQZM — on current **physical-device Mobile Safari**, focus the dedicated drawer
   terminal and confirm it occupies the complete visible screen with only the Exit control
   remaining. Open the software keyboard and verify the terminal's bottom edge and final row
   stay above the keyboard while its top edge follows any shifted visual viewport; rotate the
   device and repeat. Tap Exit and confirm the ordinary drawer chrome returns with focus on the
   selected terminal tab. Automated Chromium coverage exercises the same VisualViewport state
   transitions, but cannot reproduce iOS keyboard composition and safe-area behavior exactly.
9. HS2-KFBRSB — on a **physical phone** (iOS Safari and Android Chrome), fill a dedicated drawer
   terminal and a magnified terminal with scrollback (for example `seq 500`). Drag down and up
   with one finger and confirm the terminal scrolls smoothly, a flick glides and stops on the next
   touch, a tap still opens the keyboard without scrolling, and the page itself never rubber-bands.
   Run `less` or `nano` and confirm a drag moves through the file. Automated Chromium coverage
   drives synthetic DevTools touch drags; only a device proves the gesture feel.
10. HS2-CKS78M — on a **physical phone** (iOS Safari and Android Chrome), open a phone terminal with
    the soft keyboard up and confirm the key bar sits directly above it and tapping any key never
    dismisses the keyboard. In a shell: Ctrl then `c` interrupts `sleep 100`; Tab completes; ↑ recalls
    history; Alt then `b`/`f` moves by word. In `vim`: Esc leaves insert mode; arrows move; Fn → F1
    opens help. In `htop` or `mc`: F-keys and PgUp/PgDn work. Double-tap Alt to lock it, confirm the
    underline, and confirm leaving the terminal clears it. Automated coverage asserts the exact bytes
    and focus retention in Chromium; only a device proves keyboard retention and IME interplay.
11. HS2-FRB545 — on a **physical phone** (iOS Safari and Android Chrome), over both HTTPS and the
    plain-HTTP LAN host: in drawer focus mode and the magnified terminal, tap **Copy**, long-press to
    select part of the text with the native handles, tap **Copy**, and paste into another app to
    confirm only the selection was copied; repeat with nothing selected for the full text. Copy text
    in another app, tap **Paste** (accept the iOS Paste confirmation) and confirm it reaches the shell;
    deny it (or use the HTTP host, where the API is missing) and confirm the paste sheet opens, the
    native Paste callout works in its field, and **Paste** sends it with the keyboard returning to the
    terminal. With the keyboard up, confirm Fn shows the Clipboard group and using it keeps the
    keyboard up. Automated Chromium coverage grants or stubs clipboard permissions; only a device
    proves native selection handles, the iOS paste prompt, and the legacy copy fallback.
12. HS2-KKP8YJ — on a **physical phone** (iOS Safari and Android Chrome), hold one finger still on
    a drawer terminal (before and in focus mode) and on a magnified terminal. Confirm the
    word under the finger is highlighted after about half a second, with no iOS callout, loupe, or
    native text selection and no Android system menu, and that lifting opens the Copy / Copy Text… /
    Paste menu without raising the keyboard. Hold on a word and drag across lines before lifting;
    confirm the highlight follows the finger and **Copy** puts exactly that range on the clipboard
    (HS2-EYR96N). Hold the drag at the top edge and confirm the terminal scrolls back through
    earlier output while the highlight grows, at a controllable speed (HS2-4BARC8). Hold on blank
    space and confirm the menu offers only Copy Text… and Paste. Choose
    **Copy Text…** and confirm the Copy sheet opens. Long-press again, choose **Paste**, and confirm
    the clipboard reaches the shell. Confirm a slow drag still scrolls without opening the menu, and
    a quick tap still focuses the terminal. Automated Chromium coverage drives DevTools touch
    events, which cannot prove WebKit's callout suppression or the gesture's feel.
13. HS2-5DHHPV — on a **physical touch tablet** in landscape (an iPad without a trackpad, and an
    Android tablet), open a project terminal in the drawer and confirm Copy and Paste appear in the
    rail beside Hide drawer. Copy opens the sheet; Paste reaches the shell after the iOS paste
    prompt. Magnify a grid terminal and confirm its footer carries the same pair. Attach a trackpad
    or mouse to the iPad and confirm the pair disappears. Automated coverage emulates a coarse
    pointer in Chromium at 1180x820, which cannot prove Safari's pointer media reporting.
14. HS2-8JPRRC — on a **physical iPhone** (iOS Safari), tap into New ticket → Details, the
    inspector's details editor, the Add tag field, an attachment purpose select, and the ticket
    source repository search. Confirm the page never zooms in on focus. Automated coverage measures
    computed font sizes under Chromium phone emulation, which cannot prove Safari's zoom heuristic.
15. HS2-Q5TRYD — on a desktop with **classic (always-shown) scrollbars** (macOS "Show scroll
    bars: Always", or Windows), open an AI conversation with a permission request whose command is
    long enough that the card is taller than the dialog (shrink the window). Drag the foreground's
    scrollbar with the mouse and confirm it scrolls to Allow Once. Then use a short command that
    fits and confirm the transcript beneath the foreground stays clickable. Automated coverage runs
    with overlay scrollbars (no gutter), so it asserts pointer interactivity and hit-testing, not
    the drag itself.

### Local production host over the LAN

HS2-587N4D — run `npm run prod:rebuild-lan` in `clients/web`, then open
`http://<machine-LAN-address>:4175/` on a phone. Confirm the app loads quickly, a project opens, a
terminal attaches and streams, and live ticket changes arrive. Stop the host with Ctrl-C and
confirm the machine server keeps running. Automated coverage runs the real host on loopback only.

HS2-EFEWSC / HS2-6VQ5RV — after an explicit production rebuild and relaunch, keep the
host and server running while editing a Rust source file in the disposable checkout
where its binaries were built. Rebuild the web client and Rust executables in that
checkout without relaunching the host.
Refresh one client and open the project in a second browser. Both should remain connected
without a false build-mismatch or setup-template banner, and the second browser should still receive the
original client bundle. Restore the source file, then deliberately rebuild and relaunch;
confirm the new build is selected. Automated snapshot tests cover overwritten and
deleted working artifacts; the real LAN transition remains a manual check.

### Browser identities on ordinary LAN HTTP

HS2-76ZR5P — on physical Mobile Safari at the server's LAN HTTP address, create a ticket
with multiple attachments, cancel another draft then create again, draw two annotations,
regroup attachments, and open linked ticket readers. Add, rename, remove and recreate a
workspace visibility group. Create two AI chats, send turns, and reopen a saved conversation
twice. Confirm both new objects remain independent, each attachment gesture shares its batch,
and error/retry flows remain usable. Repeat after reload and over HTTPS. Automated WebKit
coverage checks the real insecure-origin capability boundary; device suspension remains a
manual check.

### Safari long-session memory (HS2-2G23X9)

In desktop Safari, identify the WebContent process for a Hot Sheet tab before sampling
it; the process name alone is insufficient when several tabs are open. Keep a
representative project open with active terminals and edit tickets over an hour or
until a tab reset occurs. Record the sequence, elapsed time, per-tab process RSS or
physical footprint at regular intervals, an idle interval, and any Safari memory
warning or reset. Repeat after a reload with the same project. The opt-in Playwright
WebKit profile in `docs/21-scale-stress-testing.md` exercises a disposable real
server and PTY, but cannot attribute an existing Safari tab or reproduce its
background and extension behavior.

### Real-device mTLS enrollment

1. Enroll a second physical device against an off-loopback server.
2. Verify read-only and read-write ACLs independently.
3. Revoke and renew certificates and confirm enforcement without restarting the server.

### Native client UX

Exercise platform accessibility, background/resume, notification presentation, and
credential storage once the native clients exist.

### Mobile paged board touch swiping (HS2-ZYJMDP)

On a **physical phone** (iOS Safari, the installed PWA, and Android Chrome), open a project in
the Columns view. Swipe partway toward the next column and lift: the board should animate onto
the nearest column's leading edge (the next column when past halfway, otherwise back). Flick
quickly: momentum should settle on a column start, never between columns. Confirm that vertical
scrolling inside a column is not hijacked by the horizontal snap, that tapping a ticket still
opens the inspector overlay, and that rotating the device re-snaps to a column. Playwright
covers wheel/trackpad snapping in Chromium; real touch momentum is manual.

### Mobile keyboard leaves the app at its origin (HS2-BCA512)

On a **physical iPhone** (Safari and the installed PWA), open an image attachment, choose
Annotate, draw a mark, type in the Annotation note, then dismiss the keyboard (tap the image or
Finish markup). The page must not zoom in when the note is focused, and once the keyboard closes
the app header must sit flush at the top with no empty band at the bottom. Repeat with another
text field (ticket title, search). Playwright pins the 16px coarse-pointer note size and the
root-scroll restore with a simulated offset; real iOS keyboard scrolling and focus zoom are manual.

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

### Command edit survives an immediate reload (HS2-25HAK3)

Playwright cannot route the `keepalive` request a real page unload sends, so the browser test
dispatches `pagehide` before reloading. In the running app, open Project Settings → Commands,
add or delete a command, and reload (or close the tab) within half a second. After reopening,
confirm the edit is still there.

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

### OpenCode session push in a real TUI (HS2-PEJ962)

Open an OpenCode AI terminal from Hot Sheet (Connect, or `POST /terminals {"connect":"opencode"}`),
then send `POST /connections/<terminal-id>/message` with `{"text":"…","banner":"…"}` and with
`{"command":"agent_cycle"}`. Confirm in the attached TUI that the banner toast appears, the prompt
appears in the session and a turn starts, and the agent indicator switches (Build → Plan).
Automated tests replay the protocol but cannot see the rendered TUI. Last verified on
2026-10-17 with opencode 1.17.18 (spike PTY capture plus a live Hot Sheet launch).

### OpenCode Drive command dispatch (HS2-AE37AJ)

OpenCode 1.17.18 exposes the Hot Sheet skill as `hotsheet` in `GET /command` for this checkout.
In a disposable project with a harmless `hotsheet` skill, `POST /tui/execute-command` with
`{"command":"hotsheet"}` returned `200 true`, but the attached TUI did not start a session and
`GET /session?roots=true` remained empty. Do not treat that response as confirmation of skill
execution. Drive sends its normal prompt through `prompt_async` until a direct command path
has an observable completion signal and a live TUI check.

### Codex first message (HS2-8R7SN0)

Open a new Codex AI terminal in Hot Sheet and, before typing into it, send a prompt through
`POST /connections/<terminal-id>/message`. Confirm it appears as one submitted prompt,
including when it contains multiple lines. Try another message before the SessionStart hook
reports, and confirm it is refused rather than submitted twice. After the hook reports,
send a second message and confirm Codex receives it through `codex-queue`. Repeat with a
reattached terminal and a shell terminal to check that neither gains a fresh bootstrap.

### Codex interactive permission-hook compatibility

Run `HOTSHEET_CODEX_LIVE=1 cargo test -p hotsheet-cli
launch_codex_interactive_permission_contract -- --ignored --nocapture` with authenticated
Codex CLI credentials. The test installs a vetted temporary project hook, causes a real
Codex turn to request Bash approval, verifies the documented `PermissionRequest` payload,
returns a native deny, and confirms the command did not run. Production launches retain
Codex's `/hooks` hash-review gate; only this isolated drift test bypasses persisted trust.

### AI shell permission prompt in app

In the app, open a project's shell terminal, run `claude` (and separately `codex`), and ask for
a command that needs approval. The permission prompt must appear in the Hot Sheet app, not in
the shell, and the app's answer must be honored (HS2-HE4AVD). Real interactive tools can't run
in CI; the automated tests prove the route-back environment reaches the shell.

### Codex hook trust and the terminal connection state (HS2-EV1XK3)

In the app, open a project's AI shell with Codex after `hotsheet-cli setup --refresh`.

1. Before reviewing the changed hooks with `/hooks`, the drawer tab shows the warning `unplug`
   icon after about 15 seconds, and a command needing approval prompts in the terminal.
2. Run `/hooks` and trust Hot Sheet's hooks, then restart Codex. The tab shows `plug`, and the
   next approval appears in the app.
3. Exit Codex. The `plug` icon goes away.

Repeat with `codex` run by hand in a plain shell terminal (only the `plug` state applies there).
Real interactive Codex trust cannot run in CI; the automated tests drive the same hook adapter
and server routes.

### Driven chat permission modes with real tools (HS2-NHRDMW)

The mode plumbing is automated against fake drives and a scripted Codex daemon. The live
probes below cover selected Claude operations; the remaining in-app checks use an AI chat
from the terminal drawer:

Live headless validation (HS2-YAS0FR, 2026-10-22): Claude Code 2.1.307 in `stream-json`
mode ran an `auto` Read without a Hot Sheet hook bridge request. An `auto` Bash `pwd` with
an explicit native `permissions.ask` rule emitted one `PermissionRequest` and completed
after an allow response from an isolated local bridge. A `manual` Write likewise emitted
one request and completed on allow. An `auto` POST to a disposable local server ran without
an ask; Claude classified it as allowed. The actual in-app popup and an action naturally
classified as risky remain manual checks below (`HS2-KMNQ21`).

1. Claude under `Accept edits`: ask for a file edit (no popup) and a shell command (popup).
2. Switch the chat to `Auto` from the model menu's Permissions submenu: a harmless read-only
   command runs without a popup, and a risky one still reaches the Hot Sheet popup
   (validating this is `HS2-YAS0FR`).
   On Claude Code 2.1.307, also exercise a temporary explicit native ask rule for a harmless
   command: confirm the corresponding `PermissionRequest` reaches Hot Sheet exactly once,
   its allow and deny answers are honored, and disconnecting the bridge denies the ask.
3. `Plan only` makes no edits; `Bypass permissions` asks for confirmation first and then runs
   without popups.
4. Codex under `Ask when needed`: an action outside the workspace sandbox reaches the popup;
   under `Never ask` it fails instead of asking.

Each chat tab's shield names the active mode throughout.

### AI completion confidence rubric compliance (HS2-DWTJ43)

The storage, write surfaces, and display are automated; whether a live agent follows the
prompt-level rubric is not. After refreshing a project with `hotsheet-cli setup --refresh`,
have Claude and Codex each complete a small ticket and confirm the completing note has a
`## Confidence` section with the integer and one high/medium/low line per factor (request
clarity, context, verification realism, scope deviation/assumptions, deferred gaps), that
the same integer arrived as structured `confidence` metadata (`hotsheet-cli show` prints
`confidence: NN`), and that the web inspector shows the matching header badge.

### AI chat transcript selection with a screen reader (HS2-S9W63Q)

Playwright asserts the accessibility tree. What a real screen reader announces is not
automated. With VoiceOver on macOS (Safari and the Tauri app), and NVDA on Windows if
available, open an AI chat in the terminal drawer and in the conversation dialog, then check:

- The transcript's message area is announced as the multi-selectable "Conversation Messages"
  grid.
- Each message is a row that reports selected or not selected when it gets focus.
- Cmd/Ctrl+Space and Shift+Enter announce the selection change.
- Up/Down and Home/End move between messages.
- Activity groups and skipped-message dividers read as ordinary rows without a selection state.
- Their Activity toggle remains operable.

## Mobile gallery pinch (HS2-BM78ME)

- On a physical iPhone in Safari, open an image in the media gallery and pinch outward over the media. Confirm that only the media enlarges while the top and bottom controls remain visible and usable.
- Pan enlarged media with one finger, pinch inward, then repeat after closing and reopening the gallery. Confirm that normal Safari pinch behavior remains available outside the media stage.

## Phone Workspace Grid fit (HS2-7VV2KN)

- With two live terminals on a physical phone, including one first opened in the drawer, open the Workspace Grid. Confirm that one full-width 80×24 preview appears per row, that both terminals remain reachable by vertical scrolling, and that tapping a card still opens the magnified terminal. The drawer terminal must keep an 80×24 PTY claim while shown in the phone grid.
- Zoom out to two cards across, reload, and confirm the phone preference persists. Zoom back in; switch to a desktop viewport and confirm its grid fit is independent.

## Rulesync local policy migration review

Follow `guidance/rulesync/README.md` with a disposable copy of a real project that has
handwritten guidance and nested `hotsheet:specifics`. Select Rulesync ownership before
generation, retain backups, and review the automatically saved
`.rulesync/rules/project-local.md`. Confirm it includes unmarked prose and nested
specifics, and excludes managed reusable sections. Review customized managed text
separately before generation.
Review dry-run output against the backups, including customized managed-block text.
Confirm one Up Next consolidation ticket appears after the switch, repeated switches
and applies reuse it while open, and a later change after completion creates a new
task. Update `AGENTS.md` or `rulesync.lock` directly, run the standalone Rulesync
status check, and confirm it queues the same review flow. Confirm the task never
copies project prose into its body.
After generation, verify the chosen reusable sections appear once, local policies
remain readable, CLAUDE imports AGENTS, and Hot Sheet refresh leaves both files unchanged.
Make a further local-rule edit and generate again. Rebuild local-only guidance and
return to builtin ownership; verify that edit survives and integrations still work.

Automated coverage checks representative fresh/existing fixtures and all writer
transitions. This manual review covers the human judgment needed to classify arbitrary
project customizations during an actual migration.

## Automated Coverage Summary

- Terminal sizing policy transitions and disconnect healing are automated in Rust and
  server WebSocket tests; only the real multi-device presentation remains manual.
- mTLS certificate/ACL behavior is automated; only physical-device enrollment UX remains.
- ACP wire parsing has scripted unit and contract-fixture coverage; the provider-backed
  OpenCode smoke above guards executable/provider integration drift.
- Codex hook generation, allow/deny/fallback/retry mapping, timeout behavior, setup merge
  safety, and subdirectory launch are automated without credentials; the gated provider
  smoke above guards the installed Codex lifecycle contract.
