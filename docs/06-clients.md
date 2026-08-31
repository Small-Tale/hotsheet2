# 06. Clients

> **Status: Confirmed** (maintainer, revised 2026-08-26). Sequencing:
> **1) browser web UX → 2) Tauri host → 3) native SwiftUI macOS → 4) native SwiftUI iOS → 5) Android
> (Kotlin/Compose).** **Every client is a pure API consumer — no client embeds the
> core.** The server is always a separate process (local included), which the
> client auto-starts if needed and which outlives it.

## 6.1 Principle: clients are views over the service

Every client is a consumer of the same service API (HTTP + WebSocket + MCP). No
client holds authoritative state and **no client embeds `hotsheet-core`** — they
all talk to a running `hotsheet-server`. Two clients on one project show the same
thing because the server is the single authority.

The web client keeps a separate undo/redo history per checkout. Inspector field
edits, Up Next toggles, and sidebar drops are individual transactions. Undo changes
only fields that still equal the local operation's after-state, so an interleaved
remote update wins while untouched fields can still be restored; redo uses the same
field-aware rule, and a new edit after undo clears redo.

Following HS1, Cmd/Ctrl+C and X retain structured selected-ticket data while writing
readable text to the system clipboard. Cmd/Ctrl+V copies the ticket content and uses
` (Copy)`, ` (Copy 2)`, and so on for case-insensitive title collisions; cut originals
are deleted only after destination creation, note creation, and provider-qualified
attachment copying all succeed. A paste is one history transaction: undo archives every
created copy and restores cut originals, while redo reapplies the complete transfer.
Failed transfers archive partial destination tickets and leave cut originals intact.
These global shortcuts yield to
editable controls and dialogs. Dragging an unselected ticket moves only it, while
dragging a selected ticket moves the selection; Queue, Backlog, and Archive sidebar
destinations apply the corresponding status and visibly highlight during dragover.

This is the clean client/service split the rewrite is chartered to create, made
**absolute**: the server is a standalone process even for local use, so the client
is only ever a view. Sharp contrast with HS1, where the server rendered HTML via a
custom JSX runtime, the client re-derived logic in a hand-rolled `kerfjs` SPA, and
the Tauri app *owned* a Node sidecar that died with it.

## 6.2 One access model: talk to a server (local or remote)

There is **one** way a client gets data — over the API to a server. "Local" vs.
"remote" is only *which* server:

| | **Local project** | **Remote project** |
|---|---|---|
| Server | A **localhost** `hotsheet-server` (the client auto-starts it if absent) | A server on another device/machine |
| Transport | HTTP/WS on loopback (+ secret; mTLS optional) | HTTP/WS over **mTLS** |
| Who runs it | This machine's one shared server instance | That device's server |

A client can show local and remote projects side by side (tabs); each tab carries
its server's `(origin, secret)` — carried from HS1's multi-server remote-client
design (§112). A local tab's origin is simply `https?://127.0.0.1:<port>`. There is
no `dataDir`/embedded-core tab kind any more.

**Auto-start + independence (the key behavior).** On launch a desktop client
resolves the local server via `~/.hotsheet/instance.json`; if none is running it
**spawns one detached** and connects. The server then **keeps running after the
client quits** (in-flight AI work and terminals survive). Full lifecycle:
[04-core-server-cli.md](04-core-server-cli.md) §4.3.1.

## 6.3 Web client and Tauri desktop host

- **Web first.** Build and iterate on the Kerf web client in a normal browser before
  adding its Tauri host. A `/ux-demo` route renders the real production components
  against deterministic mock service adapters for isolated and composed UX review.
  Keep the platform-neutral component responsibilities close to the planned macOS
  SwiftUI architecture; share concepts and API contracts, not rendering primitives.

- **Real local web entry point (initial implementation, HS2-0P1MDG).** `/` renders the
  production AppShell over checkout-scoped server APIs; `/ux-demo` remains the isolated
  development catalog. The project-tab `+` action opens a code-checkout dialog. On first
  open, the server conservatively discovers a valid sibling `<checkout>.hs2` git ticket
  store, hosts it, and records the many-to-many checkout/store link. The dialog accepts
  an explicit git-store path when the convention does not apply. The Vite-only bridge
  discovers or detached-starts the local server and keeps its bearer credential out of
  browser state; Tauri will replace that bridge with its native lifecycle layer.

  The bridge performs the authenticated compatibility handshake when attaching to a
  discovered server. The client distinguishes compatible skew, client-too-old,
  server-too-old, and unknown metadata. Compatible revision differences do not block use;
  an old client offers reload/update, while an old server is surfaced without an unsafe
  restart action unless both restart and quiescence capabilities are explicitly present.

  Ticket-provider connections are not stored in `hotsheet-settings.json` or
  `hotsheet-settings.local.json`: those remain shared/local preferences. Git sources are
  checkout/store links in the machine registry, while external provider connections are
  non-secret records in the ticket store's `providers.json` (credentials remain keychain
  references). Full multi-source editing in the real Settings view is tracked separately;
  the initial view reports the active source paths and their storage model.

  The real inspector's attachment surface materializes ordinary-sized browsed and
  dropped files before upload, so a macOS promised screenshot cannot disappear while
  `fetch` lazily reads it. Empty, unreadable, and short-read files are rejected with
  actionable guidance while valid siblings continue. Accepted files go to the selected
  ticket's checkout-scoped attachment endpoint, then the client refreshes both the
  selected ticket and project rows from the authoritative response.
  When the selected provider advertises attachment support, each attachment exposes
  icon actions to open, download, copy its checkout-qualified reference, or remove it;
  upload/removal progress and failures remain visible in the attachment panel. Browser
  clients use download where a native Tauri host can later offer Reveal in Finder.

  The default `Queue` view is the active working set and intentionally excludes both
  Backlog and every terminal/archive status. Backlog and Archive are disjoint explicit
  views with counts derived from those same predicates.

- **Rust shell + web UI, no embedded core.** The Rust shell's job on the server
  front is to **launch and supervise the local `hotsheet-server`** (spawn it
  detached if `instance.json` shows none, watch its health) — not to run the core
  in-process. The web UI talks HTTP/WS to that server exactly like the remote case.
  Remote projects use the Rust-side mTLS proxy already designed and scaffolded in
  HS1 (§112.5.1 — a loopback proxy that presents the device cert, sidestepping
  every WebView's broken client-cert handling).
- **UI framework: Kerf (`kerfjs`)** (maintainer, 2026-08-22 — revises the earlier
  "small mainstream framework / not kerf" lean). The web UI uses **Kerf**, the
  maintainer's own fine-grained-signals + JSX framework (~12 KB, no vDOM, no
  compiler): `signal`/`array-signal` for live WS-driven ticket lists, `ref`/`scope`
  for imperative widgets like the xterm terminal, tree-shakable list virtualization
  (kerf 4.2), and a planned tree-shakable router. Since HS1 the framework has matured
  into a published, well-tested v4 — so the "don't re-hand-roll a runtime" concern
  that pointed away from it no longer applies, and dogfooding HS2 on kerf keeps the
  whole self-hosting loop (agents included — kerf ships an AI skill + `llms.txt`) in
  tooling the maintainer owns. Client-local, revisitable, and it does not affect the
  service. (Standing caveat: single-maintainer bus factor, mitigated by it being
  dogfooded by that same maintainer.)
- **Component library: Web Awesome Core on top of Kerf** (validated 2026-08-25).
  Kerf owns state, routing, lists, API resources, and delegation; framework-neutral
  Web Awesome custom elements own accessible controls, dialogs, drawers, menus, and
  related UI primitives. Components are pinned npm dependencies, cherry-picked, and
  bundled locally for offline Tauri use. The executable spike in
  `spikes/kerf-webawesome/` proves custom-element identity, value, and focus survive
  Kerf morphs; theme tokens work; lifecycle events delegate; and the production
  bundle makes no external requests. Web Awesome 3.11 form controls emit host-level
  standard `input` / `change` events—not `wa-input` / `wa-change`; component lifecycle
  events retain names such as `wa-show` / `wa-hide`.
- **Iconography: Lucide only** (maintainer, 2026-08-26). Decorative and symbolic
  UI never uses emoji or font glyphs as stand-in icons. All clients share the Lucide
  metaphor and render official Lucide assets through a platform-appropriate shared
  component. Adjacent text owns accessibility meaning; icon-only controls have an
  explicit accessible name. Ask before choosing when multiple Lucide metaphors are
  materially plausible.
- **Platforms:** macOS primary; Linux/Windows via the same Tauri pipeline as HS1
  (best-effort, community-tested).

## 6.4 Native SwiftUI client (macOS + iOS)

- **Native API client** talking HTTP/WS to a `hotsheet-server` — **not** an
  embedded-core app. No `uniffi` bindings are needed (they were only for the retired
  embedded-core plan).
- **Why native (not just Tauri on iOS):** a first-class iOS experience — real
  navigation, share sheet, notifications, widgets — and macOS menu-bar integration.
  The identical domain behavior across surfaces comes from every surface talking to
  the *same server*, not from sharing a linked library.
- **macOS:** auto-starts + supervises the local server (like Tauri, §6.3).
- **iOS is remote-first — and structurally so.** A phone can't run an independent
  background server (iOS background-execution limits), and it rarely hosts the git
  repos or drives AI tools anyway. Its job is to view/triage tickets and answer
  permission prompts against a server running on a Mac — the **remote-server** path
  (mTLS, QR pairing — §112.6). There is no local-server-on-iOS mode. See
  [08-distributed-and-remote.md](08-distributed-and-remote.md).
- **Terminals/AI-drive on mobile:** out of first scope. Mobile watches and
  triages; driving AI tools stays on the desktop/server. Answering permission
  prompts and reading busy state *do* work on mobile (they're just API events).

## 6.5 Android (last)

A Kotlin/Compose **API client** (HTTP/WS), fourth and last in the sequence. Like
iOS it's a pure server consumer — no core embedding, no JNI bridge needed. Only a
view layer is new work.

## 6.6 What every client must render (feature floor for v1)

- Bullet-list ticket entry + the list/column views, categories, priorities, the
  7 statuses, up_next, tags, notes, attachments.
- Live updates over WebSocket (index changes, claims, busy state).
- The AI-drive surface: launch/trigger a tool, the **permission popup**, the
  **busy indicator**, the connection count.
- Multi-project tabs (local + remote).
- Search (FTS) and filtered views.

The long tail of HS1 UI (custom views/query builder, terminal dashboard, stats,
Announcer, telemetry dashboards, print) is **deferred**, each its own ticket after
the floor lands.

## 6.7 Terminal display & multi-viewer PTY sizing

> **Status: the server-side arbiter is built (HS2-BD7Q74).** The cross-device
> generalization of HS1's terminal "borrow-stack" (docs/54), which worked locally but was
> never designed for remotes. `hotsheet-terminals::SizeArbiter` implements the model below —
> leased viewport claims, focus-follows (default) + smallest/largest/pinned, the
> `SIZE_FOCUS_HOLD`/`MIN_DELTA`/`RESIZE_MIN_INTERVAL` guards, and disconnect self-heal — wired
> into the WS attach (Text `{resize}` claims in, `{pty_size, driven_by}` decisions out). The
> **client-side** viewport rendering (§6.7.4 letterbox/scale-to-fit, "tap to resize") lands
> with the client work.

### 6.7.1 The fundamental constraint

A PTY has **exactly one size** (cols × rows) at any instant. Resizing it sends
`SIGWINCH`, and the program inside (claude, vim, a TUI) **reflows to that size** —
so resizing is disruptive and must be rare and deliberate. Meanwhile many
**viewports** may show the same terminal at once, each a different size:

- several views on **one** device (the drawer terminal, a dashboard tile, a
  magnified view — HS1's borrow-stack case), **and**
- views on **different** devices at once (a macOS window *and* an iPhone).

You cannot give each viewport its own native size of the *same* session: a single
PTY emits one size's worth of output, and an alternate-screen TUI was drawn for one
grid — it can't be losslessly re-flowed to another (only line-wrapped scrollback
can). So the model is **one arbitrated PTY size + graceful handling in every other
viewport** — the same reality tmux lives with. (If per-viewer native size is ever
truly needed, that's a *separate PTY per viewer* — a different shell, not this
shared session — see §6.7.5.)

### 6.7.2 The model: the server arbitrates, viewports make *claims*

The server owns the PTY, so it is the single arbiter of its size — matching the
"server is authoritative" principle and, crucially, giving **one** coordination
point for local *and* remote viewers. Each viewport registers a **size claim** over
the terminal WebSocket and keeps it alive with a heartbeat:

```
viewer → server:  { viewerId, cols, rows, focus: bool, visible: bool, activityAt }
server → viewers: { ptySize: {cols, rows}, drivenBy: viewerId }   // broadcast on change
```

- `viewerId` is **per viewport, not per device** (`<clientId>:<paneId>`), so
  intra-device and cross-device viewports arbitrate uniformly — this *is* the
  borrow-stack, generalized to every viewport everywhere.
- Claims are **leased** (reusing the claim/lease pattern, [05](05-ai-tool-plugins.md)
  §5.7): a viewport heartbeats; on disconnect (a phone that drops off Wi-Fi) its
  claim **expires** and the server recomputes size from the survivors — so a gone
  viewer never pins the PTY to its size forever. This is the piece HS1 never had.
- The server broadcasts the resulting `ptySize` to **all** viewers, so everyone
  agrees on the real size and each renders within its own viewport (§6.7.4).

### 6.7.3 The sizing policy: focus-follows, with hysteresis

Default policy (= tmux `window-size latest`, which is exactly the maintainer's ask —
"right-sized based on whichever device and view area had most recent focus"):

- **The PTY follows the size of the viewport that most recently held input focus.**
  When focus moves from the big macOS pane to the small iPhone view, the PTY
  resizes to the iPhone (after the guards below); when focus returns, it resizes
  back. `activityAt` breaks ties if two devices both believe they're focused.
- **A focused, actively-typing viewport's size is locked in** — a background device
  cannot resize the PTY out from under someone mid-keystroke. To change the size,
  take focus (which transfers the size).
- **When nothing is focused, hold the current size** (don't resize on mere
  visibility changes) — glancing at a terminal from a second device must not reflow
  it.

**Anti-thrash guards** (named so implementation has targets; tune later):
- `SIZE_FOCUS_HOLD_MS` (~500 ms) — a newly-focused viewport must hold focus this
  long before its size is applied (kills ping-pong when focus flickers).
- `SIZE_MIN_DELTA` (≥2 cols/rows) — ignore sub-threshold differences.
- `SIZE_RESIZE_MIN_INTERVAL_MS` (~750 ms) — rate-limit actual PTY resizes; coalesce
  bursts.

**Alternative policies (configurable per terminal), for when focus-follows isn't
wanted:**
- `smallest` — size to the smallest *visible* viewport so everyone sees the whole
  screen without scroll (tmux's default; good for "we're both watching").
- `largest-visible` — one big screen drives; small screens observe (scroll/scale).
- `pinned` — a fixed size the user sets; all viewports letterbox/scroll. Good for
  recording or maximum stability.

Recommend **`focus` as the default** (it's the described need) with the guards
above, and expose the alternatives as a per-terminal setting.

### 6.7.4 Rendering when a viewport ≠ the PTY size

Every non-driving viewport reconciles its viewport against the broadcast `ptySize`:

- **Viewport larger than the PTY** → **letterbox**: render the grid at its true
  size within the pane (centered / top-left), padded with the theme background.
  Never stretch. (HS1 already handles the gutter/padding — §22.6.)
- **Viewport smaller than the PTY** → **scale-to-fit then scroll**: shrink the font
  toward a readable floor to fit; below that floor, scroll within the pane. A phone
  glancing at a desktop-sized terminal scales to fit for reading; to *interact* it
  takes focus and the PTY resizes to it.
- Show a subtle affordance when a viewport isn't driving the size (e.g. "viewing at
  120×40 — tap to resize to this screen") so the mismatch is legible, not confusing.

### 6.7.5 Escape hatch: a per-viewer *separate* terminal

When someone genuinely needs a natively-sized terminal on each device
simultaneously, that's **not** one shared session — it's **separate PTYs** (the
multi-terminal model, HS1 §22.17). Each is its own shell/program at its own size,
no arbitration needed. Hot Sheet supports both: *share this terminal* (arbitrated,
this section) vs *open my own terminal* (independent). The arbitration only governs
the shared case.

### 6.7.6 Why this beats HS1

HS1's protocol (§22.9) let any client send `{resize, cols, rows}` and took
"max-of-attached or last-resized" — an implicit, race-prone consensus with **no
notion of focus, no leases, and no remote testing**. A small remote either lost to
a bigger local viewport or won by a last-write race and shrank the desktop
unexpectedly, and a dropped remote left a stale size. Moving to **server-arbitrated,
leased, focus-follows** claims fixes all three: intent (focus) drives size,
disconnects self-heal, and one arbiter means local and remote behave identically.

## 6.8 Notes, reader mode & editing

> **Web implementation shipped** (HS2-F3SS63). Keep HS1's reader mode + feedback
> concepts, but unify and enlarge them. Native-client parity and the local feedback-draft
> overlay/submission lifecycle remain separately tracked.

**Five note kinds, one rendering rule.** A note's `kind` ([02](02-ticket-storage.md)
§2.6 — `regular` / `activity` / `feedback_needed` / `feedback_draft` / `status`) determines how
it's shown, **not how the view was opened** (HS1's inconsistency: the same note
rendered as an editable feedback form when opened via "Provide feedback" but
read-only when opened via the reader icon). In HS2 there is **one reader mode**, and:

- **`feedback_needed` and `feedback_draft` notes always render in the feedback-editor
  style** (you can answer the ask / continue your draft) — wherever they appear.
- **`regular` and `status` notes, and the ticket `details`, render in the reader
  (read-only) style.**
- **`activity` notes render as a chronological timeline** ordered by `created_at`
  (ULID tie-breaker). Never collapse duplicate, repeated, or reversed transitions;
  each entry is historical context. Show `edited_at` when it differs from creation.

**Reader mode is a focus surface with an edit toggle.** Opening reader mode shows the
ticket's details + notes on one large scrollable surface (per the rule above).

- An **"Edit" button in reader mode** turns it into a **larger editing surface** for
  details/notes — because editing in the cramped detail panel sometimes isn't enough
  room.
- While editing details/notes **in the detail panel**, the **reader button stays
  available**; clicking it **launches directly into the larger reader/editing mode**,
  carrying the in-progress edit — so you can escalate from the tight inline editor to
  the roomy one without losing your place.

Net: one consistent reader mode, kind-driven rendering, and a smooth path from the
constrained detail-panel editor to a spacious full-surface editor.

## 6.9 Mutation feedback and reconciliation

Local ticket mutations project their renderable fields into the current list and
inspector immediately, then reconcile from the authoritative PATCH response. A
single-ticket mutation must not synchronously reload the ticket collection, selected
ticket, or repository status. Responses carry a per-ticket generation: late responses
are ignored, while the current failed request restores its captured projection and
shows the error. The client emits `hotsheet:mutation-timing` with optimistic and request
phase durations for local profiling.

CI protects the deterministic contract (one PATCH and no follow-up collection/status
GET) and the projection/reconciliation logic. `npm run test:performance` is the stricter
local browser gate: after warm application startup, its click-to-next-frame projection
must remain below 100 ms; 33 ms is the aspirational two-frame target. Network/disk/git
completion is reported separately and does not delay acknowledged visual feedback.

## 6.10 Cross-references
- UX component inventory and `/ux-demo` contract: [ux-components.md](ux-components.md)
- Server-side PTY manager that hosts the arbiter: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.4
- The server clients talk to + its auto-start lifecycle: [04-core-server-cli.md](04-core-server-cli.md) §4.3.1
- Remote/mTLS + mobile pairing: [08-distributed-and-remote.md](08-distributed-and-remote.md)
- Leased-claim pattern reused for size claims: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7
- Why clients don't embed the core / language rationale: [09-technology-decisions.md](09-technology-decisions.md) §9.2
