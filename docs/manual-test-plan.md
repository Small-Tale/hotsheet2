# Manual Test Plan

### Workers workspace (HS2-E9SR7R)

With a rebuilt client and synthetic tickets, open Workers at wide, medium, and
narrow widths, then return to wide. Confirm the integrator is first, background
agents follow their parent, each column retains its scroll position, and phone
columns page with a visible next-column peek. Activate two tickets with the same
slug in different sources and confirm each opens its own inspector. Check offered,
working, planned, handoff, remote CI, disconnected, idle, and zero-worker states;
an unavailable external source must show unknown coverage. Observe an idle minute:
the cursor long poll remains held without fixed-interval requests. Interrupt the
connection and verify bounded reconnect and cursor replay.
The HS2-1Z3T0Z browser slice now automates real-server worker-first registration,
integrator join, a late worker, and exact worker-card count in Chromium and WebKit.
Its wider independent-session lifecycle, connection failure, and real-app visual
inspection remain in this manual plan until observed and recorded on the ticket.

### GitHub webhook reachability (HS2-SGQ183)

1. With synthetic repository issues, configure a GitHub source and link it to a
   checkout. Store a distinct random webhook secret with `hotsheet-cli key set`,
   then run `hotsheet-cli github-webhook-setup` and repeat
   `PUT /providers/{connection_id}/webhook` with that same key reference. Confirm
   the shared configuration stays identical and `configured` is true.
2. Configure the repository webhook with the returned path, JSON body, secret,
   and only `issues` plus `issue_comment` events through a reachable HTTPS ingress.
   Before delivery, `active` must be false. Edit an issue and verify one cursor
   advance, an authoritative ticket refresh, and `active` true.
3. Redeliver the same GUID from GitHub's Recent Deliveries. Confirm no second
   cursor advance. Rotate the local webhook key value under the same reference;
   confirm `watch` becomes false until a newly signed delivery succeeds. Disable
   the source, send a signed delivery from another
   repository, and send an invalid signature: each must leave the cursor fixed.
4. Interrupt ingress, edit the synthetic issue, restore ingress, and manually
   redeliver the failed delivery. Confirm normal refresh or explicit replay
   overflow/full reconciliation. Check the client has zero idle network requests
   beyond its held WebSocket or long-poll request and uses bounded reconnect delay.
   Do not use customer issue content or live credentials in recorded evidence.

For HS2-2Z11A5, create a synthetic source ticket and use its context action to
create a follow-up in the shared composer. Inspect Code Review at wide, medium,
and narrow widths, then return to wide: the count and qualified target link must
remain readable and open the target. Remove the target in an isolated test store
and confirm the source still opens with a missing-target diagnostic. Repeat with
an external source ticket and confirm the unsupported relation write is explicit.
For HS2-MCGCPQ, open a synthetic Git ticket's Code Review tab and use the
Follow-Ups action "Link Existing Ticket as Follow-Up…". Search another project's
ticket, link it, link it again (expect the "already a follow-up" notice and one
row), and check the dialog at wide, medium, and narrow widths. On a GitHub, Jira,
or GitLab ticket, confirm the action is disabled and its tooltip names the provider.
For HS2-NXSMC7, repeat the link from the modal reader's Code Review tab and from a
linked reader opened from a follow-up row. Confirm the dialog opens above the reader,
Escape closes only the dialog, and the new row appears in that reader.
For HS2-CHH28E, open a synthetic Git ticket's Info tab and use Blocked By's
"Add Blocker…" to add a same-project ticket, then try the reverse dependency from
the blocker (expect the dependency-cycle refusal in the dialog). Remove the blocker
from its row, and check the section and dialog at wide, medium, and narrow widths.
On a GitHub or GitLab ticket, confirm the action is disabled and its tooltip names
the provider. For HS2-15PE59, repeat the add and remove from the modal reader's Info tab
and from a linked reader opened from the blocker row. Confirm the dialog opens above
the reader and the change shows in that reader.

For the macOS Tauri host (HS2-5R706Q), build the `.app` with
`npm run desktop:build` from `clients/web` using a Developer ID identity.
Launch a fresh signed copy, approve the first Keychain access if requested,
open and create a synthetic Git ticket project, then resize the actual app
window wide → medium → narrow → wide. Verify the server remains registered
after the window closes, the temporary client discovery entry disappears,
and a reopened window reconnects. For HS2-RKZKX7, change General's Show loading
activity setting, quit the native application, relaunch twice, and confirm both
project tabs and the setting return even when the bridge port changes. Repeat
with a second isolated app-data profile and confirm profiles do not import each
other's preferences. Previously abandoned random-port origins may need a one-time
project reopen. For HS2-791W19, stage a synthetic file in New ticket and another in
Not working, quit, relaunch, and confirm both files return with their contents; remove
one, create the ticket, and confirm the app-data `draft-files-v1` directory empties.
Validate codesign, notarization, and
Gatekeeper with [the desktop guide](tauri-desktop.md).

For HS2-E0H7S0 / HS2-Y4KA47, use an isolated native profile and synthetic Unicode
composer text exceeding 64 KiB. With a storage write held, close the actual titlebar
window and separately use the OS Quit menu: the app and bridge must remain alive
until the newest captured revision is durable. Repeat close/quit while waiting;
release the write and relaunch on a different bridge port to verify exact text.
Repeat with a failed write: the notice must leave the app open and editable, ordinary
preferences must still save, and a later close must retry the retained draft. Native
public-API event harnesses supplement this manual OS/titlebar check; they do not prove
signed packaging, accessibility interaction, or OS menu/Apple-event delivery.

Only behavior that cannot yet be exercised reliably in automated CI belongs here.
When automation lands, remove the manual-only step and record it below.

## Current manual checks

### Independent integrator handoff routes (HS2-J81Y9W, HS2-AJ7FJY)

For continuous-loop acceptance (HS2-76VMYG), preflight the actual CLI's `coordination
--help` and use the integrator's verified current executable if the installed CLI
is older. On one disposable code project/shared synthetic source, start a Claude
worker without a slug, then the Codex integrator; repeat in the reverse order. Verify
availability appears, an exact structured assignment wakes only its intended worker,
the actual ticket claim is held before code changes, and handoff/release returns the
worker to a second fresh worktree without waiting for the first ticket's CI.
Assign a findings-only synthetic ticket while that first code ticket remains in CI.
Have the worker record a regular `## Findings handoff` note with evidence, release
its claim, and hand off the note id. Confirm the integrator verifies the exact note,
claims and completes the ticket, and resolves the mailbox without a code commit,
push, or CI phase. Stop and resume during `verifying`, then confirm the same note
and session-fenced transition can finish safely (HS2-A1F615).
Confirm a released, expired, or replacement-owner integrator claim cannot replay
`verifying`, including a lease that expired earlier in the same second, and an activity note with a findings heading cannot serve as evidence.

Stop and resume the integrator while a handoff is pending; verify it discovers the
durable commit even if the prompt cue was lost or unconfirmed. End an idle worker,
observe stale availability, and confirm no in-progress ticket is silently reassigned.
Record an inspection note before explicit recovery. Distinguish blockers, active
claims (including outside Up Next), pending CI, uncovered sources, and true exhaustion.
Verify a stopped run wakes active CLI waits and each final exit withdraws availability.
Do not claim an ended independent app turn automatically resumes.

Use disposable code worktrees, a shared synthetic ticket store, and separate Codex
and Claude sessions. In each assignment, name the exact integrator recipient and
return route. For a Hot Sheet terminal route, verify its live connection advertises
`message`, then have the worker write its handoff, release its claim, and send only
the ticket slug through the CLI. Confirm the integrator reads the durable ticket
before integration. An unconfirmed receipt must be reported as unconfirmed.

Repeat with no addressable terminal (including an independent Codex Work/app
session) and with a stopped terminal. Verify the worker preserves its handoff,
releases its claim, and reports the need to resume the integrator and relay the
slug. Confirm no automatic wake or monitoring is claimed and no timer polls the
queue. Native parent-channel delivery applies only inside the same tool agent tree.

### Three-role connected rollout (HS2-7E5R4A, HS2-K1QNGC)

On an independent disposable project and synthetic ticket store, verify the
headless `setup codex`/`setup claude` and graphical setup/refresh paths expose the
same dispatcher, integrator and worker skills. Start the three roles in different
orders with the same capability-checked CLI. Have the integrator explicitly
publish a safe base and enable dispatcher authority only after all participants
upgrade. Confirm the integrator can no longer reserve, the dispatcher can reserve
two non-overlapping scoped tickets, and a released handoff frees a worker for a
new assignment while the integrator remains busy on the first. Use distinct owned
browser fixture ports for concurrent workers. Confirm stale base, open dependency,
unavailable source, expired role generation and no live dispatcher each report a
specific blocker. Reviewed shared document sections may run concurrently only
after the integrator's schema-five publication. End and restart the dispatcher;
the durable backlog and old source worktrees must remain available without a
chat cue. HS2-K1QNGC owns this connected pilot and its measured throughput result;
automated role/setup checks in HS2-7E5R4A do not substitute for it.

### Multi-collaborator attachment compaction recovery (HS2-RFJ1GN)

On disposable dedicated Git ticket stores with a shared bare remote and synthetic
attachments, have two machines or processes hold separate checkouts. Execute
compaction from project statistics on one, then confirm the second cannot push its
stale branch. Have each collaborator preserve local work, restore to the exact remote
head, run `hotsheet checkout compact <source> <project> --recover`, and resume writes.
Keep the mirror backup until both confirm recovery. No real shared store is rewritten.

### Launched-agent sandbox recovery (HS2-DVZXSD)

Launch Claude Code and Codex for a project whose ticket store is the sibling
`<repo>.hs2` directory. If the command sandbox denies `hotsheet-cli ls
--up-next` with `Operation not permitted (os error 1)`, confirm the agent
reads and updates the same synthetic test ticket through configured
`hotsheet_*` MCP tools, preserving its claim and actor. On a host where the
sandbox denies Chromium or WebKit startup with EPERM or a mach-port error,
confirm the agent requests approved unsandboxed execution for that browser
gate and reports it blocked if approval or the browser is unavailable.

### Gallery annotation conflict (HS2-8SD6XQ, HS2-KTHP6F)

With a real attachment provider, open the same image in two clients, save a markup
change in one, then finish a conflicting draft in the other. Confirm the latter
keeps its draft and reports the server conflict. Retry or discard, then confirm
the displayed ticket matches the server result. Inspect the failed-save banner
at wide and phone widths.

### GUI-launched MCP clients (HS2-PN848F)

Use a synthetic Git checkout whose `.mcp.json` already holds an unrelated server, and run
`hotsheet-cli bootstrap` or `setup claude` for it. Open the checkout in a GUI-launched
Claude client (Dock or Finder, not a shell) and confirm the Hot Sheet tools appear;
`ps eww` on the `hotsheet-mcp` child should show launchd's minimal `PATH`, and
`claude mcp get hotsheet` in the checkout should report `Local config` with absolute
paths. Move the `hotsheet-mcp` binary, reopen the project, and confirm the override
follows it. For a shared Codex, OpenCode, or Antigravity config, confirm setup prints the
limitation notice; a GUI-launched client then connects only if `hotsheet-mcp` is on its
PATH and it starts in the checkout. Record each launch's PATH, working directory, and
result in the ticket.

### Repository setup and paging (HS2-J4WR11)

- In a project folder without Git, open Repository Status at wide and narrow widths. Initialize, enter an unavailable remote, retry with a valid remote, and confirm existing files remain untracked.
- In a repository with more than one page of changed files or commits, scroll to load the next page. Switch projects while a page loads and check that the old result does not appear. Reset after an empty page and confirm new files appear.

### Window chrome follows the pinned theme (HS2-15W35F)

On the packaged macOS desktop app with the system in Light appearance, choose App
Settings → General → Theme → Dark: the title bar and traffic-light area turn dark
at once. Quit and relaunch: the window opens dark. Choose Light while the system
is Dark, then Auto: the chrome follows the system again. Repeat in an installed
Safari web app (Add to Dock) and confirm its title bar color follows the same
choices. Automated coverage asserts the meta contents and the mocked Tauri call
only; the native rendering needs this check.

### WebKit list scroll after ticket mutation (HS2-KTXBK2)

At wide and phone widths, scroll a long ticket list deep into its rows, change a
visible ticket's status, and confirm the same nearby rows remain readable without
a flash to the top. Review the before/after captures attached to HS2-KTXBK2 for
clipping, row alignment, and any unexpected jump. The browser regression asserts
the scroll offsets; this visual review checks the rendered transition.

### Codex terminal MCP attribution (HS2-MRR6Z0)

CI uses fake Codex parents; this checks an installed Codex against the user's own daemon, which automated runs must not start or stop.

1. Run `hotsheet-cli setup --tool codex` (or open the project) and confirm `.codex/config.toml` lists `env_vars` with the five `HOTSHEET_*` names and no values.
2. Open two Codex terminals in one project, each with an effort selected (in-process sessions). Confirm each tab's connection icon reads connected and that closing one tab disconnects only that tab.
3. Open a third Codex terminal with no effort and no model. Hot Sheet launches it with `--no-daemon` (HS2-4J7WCV), so confirm it connects only itself and that neither earlier tab changes state.
4. In a Hot Sheet shell terminal, run your own `codex` (on the shared daemon) and trigger a permission prompt. Confirm the prompt appears in Hot Sheet without being pinned to any Codex tab, and that the shell terminal and the earlier tabs show no hook connection change (HS2-4J7WCV).

### Native Codex terminal usage and halts (HS2-J14225, HS2-CYHAWB)

The server follows each Codex terminal session's own rollout under its `CODEX_HOME`. CI covers this with synthetic rollouts; this checks an installed Codex, whose rollout format is observed rather than published.

1. Open a Codex terminal in Hot Sheet, with no effort and again with an effort selected. From each session, claim one ticket and run a turn with nonzero token use. Confirm the ticket's Timeline lists the Codex model and tokens, once per response, and that a second turn adds only its own responses.
2. Release the claim from the same session. Confirm the Timeline totals do not change, because release capture skips responses the terminal already recorded.
3. Start a Codex terminal with an invalid model and send a prompt. Confirm the terminal shows the stopped-on-error notice with the provider message, and that the notice clears once a new turn starts after you switch to a valid model.
4. Keep a Codex terminal open for several minutes of turns and confirm the project's tokens per hour keeps moving, not only after the first turn (HS2-TWE913). Restart the Hot Sheet server while that terminal stays open (the broker keeps it), run another turn, and confirm its responses are recorded once and the tab still shows Codex as connected after its next permission ask (HS2-H8BZGY).
5. With your own `codex app-server` daemon running under the same `CODEX_HOME`, repeat step 1. Confirm the daemon gains no copy of the terminal's thread, for example in its thread list, because Hot Sheet no longer sends `thread/resume`.

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
   Credential Manager under `com.smalltale.hotsheet2/machine-secrets-v1-master`. Confirm a missing
   credential is reported as absent and other Win32 failures remain visible.
5. In a disposable home, migrate two old provider entries and an old permission
   vault by opening each once. Restart a signed development server and launch the
   app. Confirm the new launch asks for at most one machine master approval.
   The first migration launch may still ask for each old item. Check that
   `machine-secrets/` contains owner-only encrypted envelopes, no token text,
   and one native `machine-secrets-v1-master` item.
   On macOS, run `npm run sign:setup:macos` from `clients/web`, then rebuild and
   relaunch the development host twice. Allow the new signing identity once and
   count Keychain prompts on the second launch and after a second rebuild.
   Confirm `codesign --display --verbose=2` reports the same certificate for
   both snapshotted server builds; a distribution-signed binary keeps its signer.
6. Export the machine master to an owner-only backup outside the repository;
   rotate, restart, and confirm credentials and durable permissions still work.
   Simulate interruption after the new root is written, run `key master-resume`,
   and confirm all values survive. Remove the root, verify secrets fail closed,
   then restore the backed-up matching root and verify they return. A wrong
   backup must be rejected without changing the native item.
7. With the current backup verified, run `key master-cleanup-legacy --backup <backup>`
   and confirm the migrated per-account OS items are gone while the single machine
   master remains. Restart and confirm provider access and durable permissions still work.

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
7. On a disposable macOS profile, delay Keychain approval while starting a server
   with a primary and a configured extra store. Confirm `/health` responds before
   approval, remembered permissions remain unavailable, and Allow Once still answers
   a pending request. Approve Keychain access and confirm durable rules become
   available without a server restart (HS2-Q7R2RM).

### Legacy permission discard (HS2-AFSF4X)

1. With two registered projects sharing a Git ticket store, remember a disposable approval in the old store namespace. Open one project's Settings → Permissions → Legacy Answers, inspect the source's answer count, and confirm no action text is exposed.
2. Cancel once, then discard the source's answers. Confirm neither project can auto-approve the old action, both can request fresh approval, and unrelated checkout answers remain. Retry and confirm zero answers are discarded.
3. Repeat from a terminal with `hotsheet-cli permission-discard --project <checkout-id> --source <connection-id>` for the count, then `--confirm`. Remove or damage the source vault and confirm the operation fails without granting the action.

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
   HS2-1YX5VS — repeat with a TUI that queries the terminal after a resize (Claude Code,
   `htop`, or `vim`) open in the desktop bottom drawer and the phone looking at the same
   terminal. Leave both idle for a minute and confirm the TUI never redraws at a different
   size. Reload the phone and toggle its Wi-Fi: a passive reconnect must not take the size
   from the device you last typed on.
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
    history; Alt then `b`/`f` moves by word. In `vim`: Esc leaves insert mode; arrows move; More → F1
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
    terminal. With the keyboard up, confirm More shows the Clipboard group and using it keeps the
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

### Local production host over Tailscale Serve

HS2-587N4D — run `HOTSHEET_BROWSER_AUTH_MODE=remote npm run prod:rebuild` in `clients/web`,
then run `tailscale serve 4175` in another terminal. Open the HTTPS tailnet URL printed by
Serve on a phone, request access, and approve its browser session on the host Mac. Confirm
the app loads quickly, a project opens, a terminal attaches and streams, and live ticket
changes arrive. Open `http://127.0.0.1:4175/` on the Mac and confirm the local client
behavior; this browser also needs pairing while remote auth mode is enabled. Stop the host
with Ctrl-C and confirm the machine server keeps running. Automated coverage runs the real
host on loopback only.

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

### Host-approved browser pairing (HS2-7DBJMS)

Use a disposable synthetic store and a browser host explicitly configured with
`HOTSHEET_BROWSER_AUTH_MODE=remote`. At desktop, tablet and phone widths, request
access and confirm the screen shows the same request and durable server ids as
`hotsheet-cli remote sessions status` on the host Mac, without showing the
challenge. Approve read-only, continue, reload and reconnect; reads should work
while project/store writes and terminal input are refused. Check that local-only
file and desktop storage routes are absent. Approve a second read-write device,
exercise a write, revoke it from the host, and confirm an existing sync/terminal
socket closes within one second and subsequent requests fail. Repeat with
logout, rotation, expired challenge, five wrong redemptions, spoofed Origin,
missing CSRF, server restart, and a browser restart. Verify the local trusted
client and headless CLI continue without browser prompts. Do not use customer
data or capture a cookie/challenge in screenshots.

### Device grants and App Settings → Devices (HS2-85W6BE, HS2-DXM0YN)

With the same disposable remote-mode host behind Tailscale Serve, pair a browser
opened at `http://127.0.0.1:<port>` and one opened through the Serve URL. Approve
the first with `--admin --local` and the second plainly. The loopback browser
must show the native folder chooser from Open Project and open a project by path;
the Serve browser keeps the registered-project picker. Grant the Serve browser
admin from the first browser's Settings → Devices and confirm, without reloading
it, that its open Settings → Devices lists devices and can grant Host Access
(HS2-KTHQE5, HS2-A3TDNH); after granting it to itself, Open Project shows the
native folder chooser, again without a reload. Remove both grants and confirm the
page returns to the refusal and the registered-project picker. A plain session sees "Administrator
access required." Approve, reject and revoke from the Devices page and confirm
another open Devices page updates without reload. With both browsers on the ticket
list, start pairing from a third device: the admin browser's Settings choice and
Devices category show the count and a notice names the device, while the plain
browser shows neither (HS2-BZHMEV); rejecting it clears the count. Confirm a request still
redeems after 14 minutes and the proposed device name matches the browser and
platform on an iPhone, an iPad, and a Mac.

### Direct remote listener modes (HS2-W4JJJE)

Using only a disposable synthetic store, start the server off-loopback without a
transport flag and confirm it still requires an mTLS device certificate. Repeat
with explicit `--remote-transport http`, `https`, and `mtls`; the printed URL and
scheme must match each selection. For HTTP and HTTPS, pair a remote device from
a browser host configured with `HOTSHEET_BROWSER_AUTH_MODE=remote`, approve it
from a trusted loopback CLI, and verify REST, project/store, polling and WebSocket
access, read-only denial, and live revocation. Present the local bearer from a
second machine, including owner and stream routes and a forged
`X-Forwarded-For: 127.0.0.1`; every attempt must fail. Confirm HTTPS needs no
client certificate but verifies the server certificate, while mTLS still checks
the device certificate and ACL. Verify no HTTPS client silently switches to HTTP.
Treat an HTTP session cookie as visible to network observers and avoid using a
production account or customer content in this check.

### Desktop local and remote projects (HS2-BT4KN6)

With two disposable synthetic hosts advertising the same checkout id, open a
local project and then use desktop `+` → Remote project. Enter each explicit
HTTP or HTTPS address and port, confirm the shown protocol and durable server
identity, approve once from that host's trusted CLI, and open both remote
checkouts. Verify three independent tabs, server labels, ticket mutations and
change events on only the matching host. Restart one host and retry discovery;
replace its identity or revoke its session and verify that host fails closed
while the other two tabs remain usable. Review a changed binding with fresh
approval, then remove it and confirm its active stream closes and its selected
project no longer restores. Repeat after restarting the desktop to check the
machine-local selected pairs and private credential store. On a phone browser
connected to one host, confirm `+` still offers only that host's checkout list.
Inspect the rebuilt app at wide, medium and phone widths and after returning
to wide; verify the source choice, address form, project list and tab states
without clipping.

### Remote project terminals (HS2-PPHJMZ, HS2-JJKVJJ)

Use two disposable synthetic hosts; on one, register two checkouts, and give the
other a checkout with the same id. Approve the desktop's session read-write on
both. Open all three remote tabs plus a local project. In one remote tab create a
shell and an AI terminal from the drawer: each connects, `pwd` prints that
checkout's directory on its host, and typing echoes. Confirm the new terminals
appear only in that tab's drawer and grid, not in the second checkout on the same
host, the equal checkout id on the other host, or the local project; repeat from
each tab. Reload the desktop and confirm each terminal reattaches in its own tab.
Revoke the desktop's session on one host (or remove its binding) and confirm that
host's terminal panes disconnect within a few seconds while the other host's
terminals keep streaming. Re-approve the device read-only and confirm terminals
list but creating one reports the server's read-only refusal.

### Real-device mTLS enrollment

1. Enroll a second physical device against an off-loopback server.
2. Verify read-only and read-write ACLs independently.
3. Revoke and renew certificates and confirm enforcement without restarting the server.

### Native client UX

Exercise platform accessibility, background/resume, notification presentation, and
credential storage once the native clients exist.

For the desktop notification path (HS2-ZP869N), grant the packaged app notification permission,
open a project and leave it in the background, then publish a synthetic scoped notice through
the local server. Confirm one OS notification appears, the correct project bell increases, and
the notice stays in Pending without an interruptive popup after Pause Notifications is enabled.
Bring the app forward, mark it read, and confirm the 24-hour history retains it. Repeat with a
second project and a recipient that does not match the checkout user to confirm isolation.

### Mobile paged board touch swiping (HS2-ZYJMDP)

On a **physical phone** (iOS Safari, the installed PWA, and Android Chrome), open a project in
the Columns view. Swipe partway toward the next column and lift: the board should animate onto
the nearest column's leading edge (the next column when past halfway, otherwise back). Flick
quickly: momentum should settle on a column start, never between columns. Confirm that vertical
scrolling inside a column is not hijacked by the horizontal snap, that tapping a ticket still
opens the inspector overlay, and that rotating the device re-snaps to a column. Playwright
covers wheel/trackpad snapping in Chromium; real touch momentum is manual.

### Standby on a real phone or tablet (HS2-EQVZTZ)

On a **physical phone and tablet** (iOS Safari, the installed PWA, and Android Chrome), open a
project and tap the moon beside the sidebar toggle. The screen must stay on past the device's
auto-lock time (Screen Wake Lock), go dark-red on black, and show only that project. Switch to
another app and back: standby is still showing and the screen still stays on. Tap anywhere: the
app returns and the device sleeps normally again. Leave a desktop in standby overnight with
reduced motion off and confirm the layout has wandered a few pixels (burn-in drift) without
clipping. Playwright covers entry, exit, scope, alerts, freshness, and layout; real wake-lock
grants and long-run drift are manual.

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
an ask; Claude classified it as allowed.

Live in-app validation (HS2-KMNQ21, 2026-11-04): `clients/web/scripts/live-claude-permission.mjs`
drives the rebuilt production client in Chromium against an isolated host and home with the
signed-in Claude Code 2.1.318 (`node scripts/live-claude-permission.mjs` from `clients/web`;
evidence in `target/live-claude-permission/`).

- **Auto:** asked for nothing. An in-project `rm -rf`, a force push, `curl ... | sh`, and an
  explicit unsandboxed network retry all ran or failed without a prompt. Claude's own sandbox
  and classifier decided each one. HS2-53RV01 keeps Auto as the default and describes it as
  allowing commands and other actions without a prompt, including changes to files or Git history.
- **Accept edits:** a Write outside the workspace reached the Hot Sheet popup. **Allow Once**
  wrote the file; **Deny** left it unwritten, and Claude reported the bridge's denial.
- **Mode shield:** the chat tab's shield named the mode throughout.

Re-run the script after a Claude upgrade.

1. Claude under `Accept edits`: ask for a file edit (no popup) and a shell command (popup).
2. Switch the chat to `Auto` from the model menu's Permissions submenu: a harmless read-only
   command runs without a popup. Claude 2.1.318 also ran every risky action tried without
   asking (HS2-KMNQ21), so do not expect an Auto popup; Claude's classifier decides.
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

## Grid tiles claim scaled 80×24 (HS2-JQCGQF)

- Run Claude Code (or another full-screen TUI) in a drawer terminal, then show the drawer project grid and the Workspace Grid on a desktop window. Confirm the tile shows a full 80-column, 24-row screen scaled to the card and that the TUI does not truncate its own lines.
- Step every zoom level with − and + in both grids and confirm the tile stays 80 columns by 24 rows at each one; only the card size changes (HS2-JWKD3W). Close the terminal in other windows or devices first: a viewer that drives the PTY more recently makes the tile mirror its size by design (HS2-1YX5VS).
- Return to the drawer tab and confirm the TUI reflows to the drawer's full width and height.

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
Repeat with no linked ticket source: Project guidance should show a pending-review
notice at wide and phone widths with the Ticket sources action and headless retry
command. Link a writable default source and confirm the ticket appears immediately
and the notice clears. Make the source temporarily unwritable and confirm linking
still succeeds while the notice remains until a successful retry.
After generation, verify the chosen reusable sections appear once, local policies
remain readable, CLAUDE imports AGENTS, and Hot Sheet refresh leaves both files unchanged.
Make a further local-rule edit and generate again. Rebuild local-only guidance and
return to builtin ownership; verify that edit survives and integrations still work.

Automated coverage checks representative fresh/existing fixtures and all writer
transitions. This manual review covers the human judgment needed to classify arbitrary
project customizations during an actual migration.

## Remembered permission owner (HS2-7T5GPN)

With two code projects sharing one Git ticket store, remember an approval from one
project and open Settings → Permissions in each. The answer must identify only its
owning code project's name and path. Link a second provider, change the default
ticket source, and reopen the preview, full dialog, and match editor; the owner
must remain the code project. An old source-scoped answer, if present before
discard, must say it is a legacy ticket-source answer with its original source
identity. In each project's live remembered-answer confirmation, verify that the
owner path, AI-session reach, and lifetime name separate facts. Repeat at desktop
and phone widths.

## Automated Coverage Summary

- Terminal sizing policy transitions and disconnect healing are automated in Rust and
  server WebSocket tests; only the real multi-device presentation remains manual.
- mTLS certificate/ACL behavior is automated; only physical-device enrollment UX remains.
- ACP wire parsing has scripted unit and contract-fixture coverage; the provider-backed
  OpenCode smoke above guards executable/provider integration drift.
- Codex hook generation, allow/deny/fallback/retry mapping, timeout behavior, setup merge
  safety, and subdirectory launch are automated without credentials; the gated provider
  smoke above guards the installed Codex lifecycle contract.

### Removed terminal message action (HS2-JVSFES)

Select a capable AI terminal in the bottom drawer. At 2048×1128, medium, phone,
and returned-wide sizes, confirm the Message AI Session button is absent and no
browser-native prompt opens. Drawer tab selection, creation, clipboard controls,
and terminal output continue to work. CLI `terminal message` and the authenticated
connection-message operation remain supported.

## Native credential smoke (HS2-SFVWN5)

Unattended isolation, artifact mismatch refusal, and cross-process synthetic
credential flows are automated; see the fixture contract in
[the testing guide](12-code-organization-and-testing.md#1276-fixtures--ci).

After desktop staging, optional `npm run desktop:test:native` verifies the staged
release backends with their real native credential port. Run this only in an
attended or disposable native credential environment, using synthetic data. It may
require OS credential authorization. The default `desktop:test` instead verifies
the staged bridge/runtime with synthetic debug backends and prints that mode; it
does not claim packaged native backend validation.

### Findings correction return (HS2-AEBJYN)

Use a disposable pool with the verified schema-three runtime; never open a live pool
until all members switch. Hand off a regular Findings note, return it with an exact
Review return note from Handoff, and repeat from Verifying after explicitly releasing
the integrator claim. Assign a NEW correction operation, preserve the original note,
and verify/complete corrected findings without an empty commit or CI. Replay the
original return after the correction claim and after completion: it must preserve
that claim and leave the original Blocked, refusing Done. Keep an unrelated code CI
record pending throughout. Confirm an older schema-two runtime refuses the disposable
upgraded pool instead of mutating returned originals.

### Browser draft attachment recovery on LAN (HS2-2HF3H4)

On a disposable rebuilt server, open the real app from an insecure LAN origin in WebKit
and Chromium. Stage synthetic binary and empty files in Create ticket; check recovery
finishes, reload and reopen, and verify names and content survive. Remove a file, cancel,
then stage another and create the ticket; verify exact attachment bytes and metadata.
Inspect wide, medium and narrow, including wide-to-narrow-to-wide, while staged and after
creation. Existing Blob/File recovery records remain readable without clearing IndexedDB;
unreadable records remain visible/removable and block submission until recovered or removed.

### In-app prompt replacements (HS2-WMZ6Z5)

In the packaged Tauri application, rename an attachment from its item menu in the inspector and modal reader. Verify Enter/blur finish, Escape cancellation, blank/no-change behavior, failure retention and a later edit. Create and cancel a command group in project settings, including a duplicate name. Option/Alt-click an AI drawer choice; verify initial values, unknown-provider validation, blank model and Provider Default effort, effort-select reset and unsupported-model disable, and that Cancel/Escape starts nothing. Run an explicitly chosen real provider only in an authorized native smoke session. Check keyboard focus and wide/medium/phone/wide transitions. No browser-native prompt should open.

### GitHub first installation (HS2-FDTJ98)

With a synthetic/new GitHub sign-in that has no installations, open Connect GitHub Issues.
Check the Install Hot Sheet on GitHub link at wide, medium and phone widths, then return
wide. Open it and verify the selected host's registered App page. After granting a
repository on GitHub, use Refresh list and confirm the repository is offered and the
link changes to Add another account or organization. Repeat for a configured Enterprise
origin; without its slug map, confirm operator instructions and no guessed public App URL.

### Repository suggestions after an asynchronous list (HS2-2055AY)

In a rebuilt real app with synthetic repositories, open Connect GitHub Issues and type
a path while the repository list is loading. Leave the suggestion list open, deliver
the list, and confirm the list stays open with the typed-path create option. Edit the
path to match a newly loaded repository and confirm the matching option is offered;
restore the typed path and submit it. Inspect the open list at wide, medium, phone,
then wide again without clipped options or lost focus.

## Remote registry foundation (HS2-1Y45MZ)

Use only disposable homes and synthetic issued device credentials. Read `remote identity` on the peer, then `remote register` with its explicit server id and a private credential bundle file. Confirm `remote list` works while the peer is stopped, reports stale cached observations and never exposes secrets or paths. `remote checkouts` must withhold usable checkouts on outage, identity change, invalid certificates and local revocation. Restart with the same home and renew a server leaf under the same CA: discovery must recover without review. Replace the machine home or CA: discovery must fail until `remote review` explicitly supplies the old revision/identity and new pairing material. Repeat registration and revocation, try a stale review revision, and confirm the authenticated service and `hotsheet_remotes` tool observe the same machine registry. Test remote device revocation separately from local registry revocation.

The synthetic automated CLI/service/MCP and TLS lifecycle cases passed on 2026-10-30. The operator enrollment checklist above was not performed with real devices or credentials. There is no rendered UI in this foundation.

### Pairing through Tailscale Serve (HS2-H4REBS)

Use two machines on one tailnet with disposable homes and synthetic projects. On the
peer, run the browser host in remote mode (`HOTSHEET_BROWSER_AUTH_MODE=remote`) behind
`tailscale serve`. On the other machine, open Remote Project → Add Server and enter
`https://<peer>.<tailnet>.ts.net` without a port or CA. Approve the request on the peer
with `hotsheet-cli remote sessions approve <id>`, press **I Approved It — Connect**, and
confirm that the peer's projects are listed. Then confirm each refusal:

- The peer's `/remote-identity` answers 401 without a session.
- A request carrying `X-Hotsheet-Secret` or `?secret=` is not authenticated by it.
- A peer with a private CA fails until its CA path is supplied.

Not yet performed with real machines. The live loopback case in
`remote-auth-server-integration.test.ts` covers the same flow on one host.

### Hook and MCP terminal attribution (HS2-HBA9Z8)

Use an isolated rebuilt server and two synthetic AI terminals sharing one directory.
After the startup grace, inspect the drawer and dashboard `missing` tooltip. Raise an
authenticated synthetic permission for the intended terminal and keep it pending:
its trusted report must already appear in `/terminals` and the change stream, while the
combined badge says `mcp_missing`. Register a live synthetic MCP lease first without a
terminal id, then for the other terminal, then for the intended terminal. Only the last
step makes the intended badge `connected`; project connectivity alone does not. Ignore
the popup presentation while retaining the pending request to inspect the badge, then
deny the synthetic request. Remove/re-add MCP presence and clear/re-report the hook;
check both directions without reloading. Repeat an ask with absent/stale terminal
attribution: the inbox may show it while neither terminal acquires a hook connection.
Inspect wide/medium/phone/returned-wide states and match the original tab component
capture. For a real incident, record the exact tooltip, terminal snapshot and read-only
hook diagnosis at the same time; synthetic state reproduction alone does not identify
that incident's cause. Never use the running user host or real customer actions.

### Primary AI session icon (HS2-YHQM67)

Open two Claude terminals on a synthetic project. Confirm both tabs show a connected plug and
only the older one shows `plug-zap` with the "Primary session" label. Close it and confirm the
other tab takes `plug-zap` without a reload; reopen a terminal and confirm the role stays.

### Codex questions in the app (HS2-D0QTGB)

In a Hot Sheet Codex terminal on a synthetic project, ask Codex to pose a question with its
question tool. Confirm the "AI waiting for your answer" notice shows its title and options,
send an answer from the notice, and confirm Codex receives it as user input and the notice
clears. Repeat answering in the terminal instead, and with two Codex terminals asking at once.

### Human ticket feedback to AI sessions (HS2-25TP3N)

Use a synthetic Started Git ticket claimed by a live AI session.
Add a human note and answer a FEEDBACK NEEDED prompt. Confirm that each committed
note sends only a short ticket reference to the claimed session and records a
delivery activity note. Repeat while the driven chat is busy, with the claimed
session unreachable, with two reachable same-project sessions, and with no
session. With two reachable terminals and no claim, confirm the note goes to the
older (primary) terminal; close it and confirm the next note goes to the other
(HS2-DB2FQ2). Confirm the chooser stays within the project/source, the start action
opens a session, and a completed-ticket Not Working report remains separate.
With the ticket store temporarily read-only, choose a session for a saved note:
the session receives one reference, a retry is refused, and after restoring
writes and restarting the server a retry still sends nothing while the ticket
gains the delivery activity note (HS2-WKRFD5).

### Announcer audio on a real device (HS2-5PSQJQ)

On a rebuilt local macOS server with a synthetic project, open Announcer and play
one retained item with Browser voice, then Mac voice on server. Check that both
are audible on the client device, stop and restart playback, select another
item, switch voices while audio is pending, and close the panel. Confirm that
only the selected synthetic item plays and no temporary speech file remains.
Repeat with an incoming synthetic live event while Live is enabled. The browser
suite uses fake audio to verify request and state wiring; it cannot hear output.

### Grid sidebar touch swipe on a phone or tablet (HS2-EZSQDF)

Open Workspace Grid with two synthetic projects on a phone (iOS Safari and Android
Chrome) and open the operations sidebar. Swipe left and right across a page:
it snaps to Gauges, then Triage, and back, and the page dots follow. Scroll a
long page vertically and confirm the dots stay fixed at the bottom. Reload and
confirm the last page returns. Kerf browser-tests a real swipe in Chromium only
(`KF-0QJ2K3`); the Hot Sheet suite covers dots, arrow keys, and trackpad scroll.

### Jira advisory process claims (HS2-BPKM76)

For HS2-2MZWZT, open the same Jira ticket in the web inspector and reader at
wide, medium, and narrow widths, then resize wide to narrow to wide. Inspect
two live worker claims, a provisional offline claim, a queued operation, a
needs-attention operation, and a malformed remote property. Confirm conflict
and uncertainty copy remains legible, automatic handoff stays paused, and
Claim/Renew/Release appear only for valid current-session state. Reconnect and
refresh to confirm the projection updates without periodic requests. Exercise
the checkout-qualified MCP status/event tools against the same linked source.

For HS2-WHW46N, leave the same synthetic Jira ticket open in two independent
browser sessions. Claim, release, and reconcile a provisional operation in one
session, and confirm the other notice updates through the change stream without
Refresh or periodic requests. Exercise conflict and reconnect while the second
notice is open; check wide, medium, narrow, then wide again.

On a synthetic Jira issue with Browse and Edit issue permissions, assign the
current credential user, then create an advisory claim through the checkout
qualified route or `provider-advisory-claim`. Confirm the property appears under
the issue's entity properties and no issue comment or notification is created.
With two independent workers, confirm concurrent live markers show conflict and
block automatic handoff. Release one exact owning operation and confirm the
remaining claim reconciles. Disconnect, admit a provisional claim, restart the
server, reconnect, and confirm replay uses the original operation id and clears
the provisional flag. Test missing Edit permission and an expired pending claim;
both must fail closed. No customer content belongs in claim markers or captures.
For v2 cleanup readiness (HS2-0QEPRW), use a timestamped operation ID, verify
Jira's `Date` header is present and within 60 seconds of the local clock, and
confirm a stale same-ID event beyond 24 hours cannot become live. A legacy v1
Release property remains present and is never treated as reclaimable.
For HS2-HTYZAG, seed a synthetic Jira issue with v2 Claim/Renew/Release
properties older than 24 hours plus two skew windows, a current other-worker
Renew, and v1 markers. Submit a new advisory write: only the old v2 keys should
be deleted, then the write should reconcile. Interrupt a DELETE and its
readback in separate runs; neither may publish the new write, and a retry must
recover. At 1,000 total issue keys with no eligible v2 key, verify the write
is refused before PUT. At more than 1,000 keys, reads stop for operator
recovery. Retain every v1 marker even when expired or released.

## Orchestration target capacity and admission (HS2-J8PRTV)

Use disposable machine homes and synthetic projects only. No rendered interface is
added by this ticket. The automated CLI/HTTP/MCP flows pass with scripted readings and
one real-host reading; this checklist needs real machines and has not been run.

1. On a machine running several AI sessions, run `orchestration capacity show
--checkout <id>` and compare free RAM, CPU and each session's `resident_mb` with
   the system's own activity monitor.
2. Set `orchestration admission set --max-sessions 1` on a paired remote machine and
   confirm `orchestration admission check --target` from the orchestrating machine
   answers `reduced` or `refused` with `session_cap_reached`.
3. Run the same checks against a Linux server (HS2-24PQ1R).

## Orchestration validation steps (HS2-WN0MXZ)

Use disposable machine homes and synthetic projects only. No rendered interface is
added by this ticket. The automated CLI/HTTP/MCP flows run real processes on macOS
and in Linux CI; this checklist needs real machines and has not been run.

1. On a paired remote machine, add a profile to the target project's
   `.hotsheet2/validation-profiles.json` that runs its real test command for several
   minutes. Run `orchestration validation run` from the orchestrating machine and
   confirm it waits to the end and records the exit code and digests.
2. Restart the remote server while such a step runs and confirm the result becomes
   `unverified` with `execution_lost` and the step is not started again (HS2-7BTV98).
3. Run a step whose command starts background processes past its timeout and confirm
   none survive on the remote machine.
4. Repeat steps 1 and 3 against a Windows server (HS2-2R32SD).

## Orchestration target tickets (HS2-AZ15GA)

Use disposable homes, synthetic projects and the always-safe internal testing
repositories only. The automated flows cover git sources on a local and a real
remote server; external providers are covered only by refusal paths. Not yet run:

1. Bind a target whose source is a GitHub, GitLab or Jira connection on an internal
   testing project. Create a ticket with `orchestration ticket create`, repeat the
   command, and confirm exactly one issue exists and carries the operation marker.
2. Update its title and status, link it from a second orchestration ticket, and
   confirm `orchestration ticket links --resolve` shows both links resolved.
3. Disable the connection on the target machine and confirm the next operation is
   refused without contacting the provider.

## Orchestration permission requests and audit (HS2-XHQ1EH)

Use disposable machine homes and synthetic projects only. The automated CLI, HTTP,
MCP and browser flows pass; this checklist covers what needs two real paired
machines or a desktop notification and has not been run.

1. Bind a target to a paired remote checkout and confirm reads work without any
   permission. Have an AI session create a target ticket: confirm the permission
   card appears in the desktop app (and as a system notification when the window is
   hidden), names the orchestrator and the target project, and offers no Allow This
   AI Session. Answer Allow Once and confirm the same call then succeeds once.
2. Answer Always Allow on a second request and confirm Settings → Permissions lists
   the rule with the target project and the orchestrator; revoke it there and
   confirm the next action asks again.
3. Revoke the pairing on the remote machine and confirm the next action is denied
   with `target_unavailable`.
4. From a second machine, request the permission, access and audit routes of the
   first with a read-write device certificate and confirm each is refused.
5. In a session launched by Hot Sheet, run `orchestration permission answer` and
   confirm `forbidden`; run `permission list` and `audit` and confirm they work.

## Orchestration target declarations and bindings (HS2-4J6V46)

Use disposable machine homes, synthetic projects and credentials only. No rendered
interface is added by this ticket. The synthetic automated CLI/HTTP/MCP flows pass;
this operator checklist has not been run with real paired devices.

1. From a provider-only registered project, use explicit `-C <configured-host-source>`
   and `--project <checkout-id>`. Read an absent declaration without creating a file;
   add a retained target ULID, rename/reorder, remove and repeat with current/stale
   revisions. Confirm tombstones cannot be dropped, reused or resurrected, and a
   rename-back rotates revision. Correct a descriptive repository hint through PUT.
2. Inspect a local checkout and a paired remote checkout. Confirm path-free candidate
   identity/capabilities, `usable:false`, no binding/reservation/manifest writes and
   no credential export. Bind explicitly with the inspected destination and expected
   revisions; perform a fresh resolve. Retarget/revoke after inspect: bind must fail
   until a new deliberate inspection/review succeeds.
3. Stop/restart a synthetic peer, renew its same-CA leaf and review a same-identity
   address move. A fresh retry recovers without target rebind. Change server/CA,
   checkout or source authority: preserve the old binding as stale/unusable and
   require old/new identity review. Test partial missing provider config, disabled
   sources, unsupported metadata and an older peer with no advertisement.
4. Exercise CLI, authenticated HTTP, and actual MCP `--path`, `--server`, `--project`
   parents through list/add/rename/remove/bind/unbind/resolve plus inspect. Check
   wrong bearer, read-only/denied roles and off-box writer denial of all binding,
   resolve and inspect operations before credential/network access.
5. Corrupt/remove the owning project registration, provider config, manifest and
   identity. Exact binding read/unbind must remain available without source/runtime
   construction or project recreation. For an existing project wrapper, stale cached
   routes fail plainly; they must not start a server or reopen the owner for cleanup.
6. Check bounded input/output, redirect refusal, mismatched post-response identity,
   atomic old/new persistence, quota exhaustion with existing-key disable, and
   interleaved delayed success/failure/review/unbind/revoke. Equal/backward clocks
   never revive an obsolete proof. No target ticket/AI/grant operation is dispatched.

## Project-local permission automation after browser reset (HS2-QAGSJK)

Use an isolated test project and browser profile. Set Project Settings → Permissions to
Auto-Allow after 5 seconds, confirm the gitignored `.hotsheet2/settings.local.json`
contains `permissions.automation`, then clear that browser profile's site data. Reopen
the project and confirm Settings still shows Auto-Allow after 5 seconds and a synthetic
permission request follows it. Open a second project with no setting and confirm it
stays Off. Change the first project from another window and confirm the original
window updates. Repeat with a denied local-file write: the UI must restore the last
saved choice and show an error, without approving a request from an unsaved choice.

## Windows in-process terminals (HS2-2TD7M7)

On a native Windows x86_64 build, start a server with a disposable store and no
`--no-terminal-broker` flag. Confirm startup reports in-process terminal hosting,
open a synthetic `cmd.exe` terminal, send input, and observe its output and close.
Restart that server and confirm the old terminal is gone, matching the documented
Windows persistence limit. Run the managed credential-cache/account tests on the
same build; a Unix broker compile error must not prevent them from starting.

## Terminal launch relay with real launchd (HS2-HYF56Y)

Automated tests use a fake `launchctl` and a relay started by the test, never the real
launchd. On a Mac logged in to its desktop, verify the launchd-specific parts by hand.

1. Use a throwaway machine home and a temporary agents directory so the agent label is
   unique and the real agent is untouched:
   `HOTSHEET_HOME=<tmp>/home HOTSHEET_LAUNCH_AGENTS_DIR=<tmp>/agents HOTSHEET_LAUNCHCTL=/bin/launchctl`.
2. Run `hotsheet-cli terminal-relay install --server <built hotsheet-server> --json`.
   Confirm the label is `com.smalltale.hotsheet2.terminal-relay.<hash>`,
   `launchctl print gui/$(id -u)/<label>` shows it running, and the reported relay
   bootstrap is `live`.
3. Run the install again. It reports `already_current` and the relay pid is unchanged.
4. Start a disposable server under a null bootstrap port. One way is a short script that
   calls `task_set_special_port(mach_task_self(), TASK_BOOTSTRAP_PORT, 0)` and then execs
   the server. Its log shows the broker was launched by the launch relay, and the
   broker's `health` is `live`. Open a terminal and run `ls`.
5. Kill the server with SIGKILL and confirm the broker is still running. Run
   `terminal-relay uninstall`, which boots the agent out. Confirm the relay is gone, the
   broker and its terminal are still running, and `launchctl print` no longer finds the
   label.
6. Run `hotsheet-server -C <store> --stop --kill-all-terminals` and confirm it reports one
   killed terminal. Remove the temporary directory.

For the packaged app, launch the desktop app from Finder. Confirm `hotsheet-cli
terminal-relay status` reports the default label as healthy and naming the bundled
`hotsheet-server`. Then quit the app and confirm that its terminals keep running.

## Resource-claim token route from a guest VM (HS2-FMHH9Y)

Automated tests reach the token route over loopback and through a router with a
synthetic non-loopback peer address, never from a real guest over TLS. With a VM whose
host-only network can reach the host, verify the exposure choice in
[36-resource-claims.md](36-resource-claims.md) §36.11 by hand. Use a throwaway
`HOTSHEET_HOME` and store.

1. On the host, run `hotsheet-cli cert init`, then start the server with
   `--bind <host-only address>:<port> --remote-transport https`.
2. On the host, run `hotsheet-cli resource acquire vms/guest-check --duration 10m` and
   pass the printed `renewal_token` into the guest as `HOTSHEET_RESOURCE_CLAIM_TOKEN`.
3. In the guest, with the server certificate trusted, send
   `POST /resource-claim-token/renew` with `Authorization: Bearer <token>` and
   `{"duration_ms": 600000}`. Confirm 200 and a later `expires_at`; on the host,
   `hotsheet-cli resource show vms/guest-check` shows the same expiry.
4. In the guest, confirm that `GET /resource-claims/vms` and `GET /tickets` with the same
   bearer token, and with the server secret, both answer 401.
5. In the guest, send `POST /resource-claim-token/release`. Confirm 200, then that a
   second release answers 403 `token_invalid` and the host shows the resource free.
6. Restart the server with the default loopback bind and confirm the guest can no longer
   connect.

## Orchestrator-started session in its target checkout (HS2-WEFJV8)

Automated tests launch the workspace fake agent, which reports the files and
environment it sees but never reads them. With a real tool, use a throwaway
`HOTSHEET_HOME`, a server store, and a second git project with its own linked store
that has never been opened in a client.

1. Declare and bind the project as a target of itself, grant `sessions_manage` for
   the tool, and store a policy that lists it (§26.7, §26.8).
2. Run `hotsheet-cli orchestration session start --project <owner> --target <id>
--operation-id manual-1 --tool claude` (repeat with `codex`).
3. Confirm the answer has `tool_setup.wrote` above zero, and that the project now
   holds the tool's instruction file and MCP configuration naming its own store.
4. In the session's drawer tab, ask the tool to list its Hot Sheet MCP tools and to
   show the Up Next queue. Confirm it answers from the project's own store.
5. Stop the session with `--force` as a person. Remove the temporary directories.

## Show in Drawer for a remote orchestrated project (HS2-6E32N4)

Automated tests cover the peer's answers over real TLS and the client's routing with
those shapes; they do not drive the desktop bridge against a second machine. Use two
machines paired as remote servers, a real AI tool on the peer, and an Orchestration
project on the first machine.

1. In the desktop app, open two projects of the peer in remote tabs, then add one of
   them to the Orchestration project (+ Add Project).
2. Start a session on it (`hotsheet-cli orchestration session start --project <owner>
--target <id> --operation-id manual-drawer --tool claude`). Wait for the tool to
   connect.
3. In Settings → Orchestration, open the project's detail. Confirm **Show in Drawer**
   is enabled and opens the session's terminal in that remote tab's drawer, not in the
   other remote tab or the Orchestration project's tab, and that the terminal echoes
   typed input.
4. Close the remote tab. Confirm Show in Drawer is disabled with its reason; reopen the
   tab and confirm it is enabled again.
5. Stop the session as a person.

## Role prompt and joining with a real tool (HS2-2DPGDY)

Automated tests deliver the role prompt to the fake agent's OpenCode-style API and
register the worker in the pool themselves. With real tools, use a throwaway
`HOTSHEET_HOME` and a target project that has a coordination pool (run
`hotsheet-cli coordination --project <target> setup` once).

1. Start a session for each of `claude`, `codex` and `opencode` as in the
   HS2-WEFJV8 steps above. Note each answer's `role_prompt`: `pending` or
   `delivered`.
2. Wait for each tool to finish starting. Run `hotsheet-cli orchestration session
sync <session-id> --project <owner>` and confirm `role_prompt` is `delivered`, and
   that the tool shows the prompt naming `hotsheet-worker` and its worker id.
3. Confirm each tool runs the worker skill and that
   `hotsheet-cli coordination --project <target> status` lists its worker id. Sync
   again and confirm `joined_at_ms` is set.
4. Stop each session as a person with `--force`. Remove the temporary directories.

## Antigravity launch prompt (HS2-KK3CCE)

Antigravity (`agy`) declares no `[launch] prompt_args` yet, so its orchestrated sessions
carry only `HOTSHEET_SESSION_ROLE` and `HOTSHEET_ROLE_SKILL` and report `role_prompt:
unsupported`. Third-party guides for agy 1.2.4 say `agy "<prompt>"` and
`agy -i "<prompt>"` (`--prompt-interactive`) open the interactive session with a first
prompt; that is unverified. With a real, signed-in install:

1. Run `agy --version` and `agy --help`; record the version and whether a positional
   prompt or `-i`/`--prompt-interactive` is listed for the interactive session.
2. In a scratch project, run `agy -i "Reply with the single word READY"` in a terminal.
   Confirm agy answers READY and then stays open for another prompt (it must not exit
   like `--print`). Repeat with `agy "Reply with the single word READY"`.
3. Repeat step 2 with a two-line prompt (a line break inside the quotes) and confirm
   both lines reach agy as one prompt.
4. If a form passes, add it to `plugins/antigravity/manifest.toml` under `[launch]`
   (for example `prompt_args = ["-i", "{prompt}"]`), rebuild, start an orchestrated
   session for `antigravity` as in the HS2-WEFJV8 steps and confirm its answer's
   `role_prompt` is `at_launch` and the tool shows the role prompt. If none passes,
   record the version and keep `prompt_args` undeclared.

## Codex bootstrap role prompt (HS2-N1GZFE)

A newly spawned Codex TUI gets the orchestrator's role prompt by a one-shot bracketed
paste into its terminal right after launch (`codex-pty-bootstrap`), which reports
`delivered` without confirmation. Whether Codex keeps a paste that arrives before its
TUI has drawn its composer is unverified. With a real, signed-in `codex`:

1. In a scratch project Codex already trusts, open a Codex AI terminal from the drawer
   and, within a second of it appearing, paste a short prompt ("Reply with the single
   word READY") and press Return.
2. Record whether the prompt appears in the composer and is answered, is lost, or lands
   in a start-up dialog (trust, sign-in or update notice). Repeat three times, and once
   in a directory Codex does not trust yet.
3. Start an orchestrated Codex session (HS2-WEFJV8 steps) and confirm the role prompt
   naming `hotsheet-worker` is shown and acted on.
4. If an early paste is ever lost or answers a dialog, the bootstrap prompt must wait
   for the first idle observation or the SessionStart report; record the outcome on
   HS2-N1GZFE.

## Orchestration end to end with real tools and hosts (HS2-2EY9DP)

The end-to-end scenarios (§26.23) run fake AI tools, a loopback GitHub Actions
provider and two servers in one process on macOS. These checks need what they cannot
have. Use throwaway `HOTSHEET_HOME` directories, synthetic projects and a scratch
GitHub repository; never a customer project.

1. **Two real machines.** Pair a second machine (Linux, then Windows) with
   `hotsheet-cli remote register` over mTLS and declare one of its checkouts as target
   B, with this machine's checkout as target A (§26.2, §26.7). Store a policy with
   `placement.automatic` and the real `claude` tool on A and `codex` on B.
2. **The loop.** Plan B after A with `--gates ci` (§26.15). Confirm a session appears
   in this machine's drawer for A, takes up its worker role and claims A's ticket.
   Push a commit for A to the scratch repository and link its CI gate with
   `orchestration gate link` (without `HOTSHEET_GITHUB_API_URL`). Confirm the gate
   goes green from the real workflow run with no further command, B's step becomes
   `confirmed`, and a session appears in the other machine's drawer for B.
3. **Outage and restart.** Disconnect the second machine's network while its session
   works B: `run status` shows `unreachable` and `stale`; reconnect and confirm the run
   catches up. Restart this machine's server while B is worked: no second session
   starts on either machine.
4. **Lost access.** Unbind B as a person: `run sync` answers `target_unavailable`,
   and nothing starts or stops on B. Bind it again and let B finish; confirm both
   sessions are wound down after `idle_minutes`.
5. **Windows host.** On a Windows target, confirm the session starts in the target
   checkout (its PTY and process handling differ, HS2-2R32SD) and that capacity
   reports `null` memory and CPU (§26.10) without blocking admission when the owner
   set `allow_unknown`.
6. Stop every session as a person with `--force`, unbind both targets, delete the
   scratch repository's workflow runs and remove the temporary directories.
