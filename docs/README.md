# Hot Sheet 2 — Design Docs

This directory holds the design for **Hot Sheet 2** — a **Small Tale Inc.** project,
and a from-scratch rewrite of the original Hot Sheet (`~/Documents/hotsheet`). It is
in **early implementation** — the Rust core, CLI, server, index, MCP shim, and
AI-tool plugins are built; see [CODEBASE-MAP.md](CODEBASE-MAP.md) for what exists.
These documents remain the source of truth for what we are building and why.

> Status legend used throughout: **Decided** (recommendation made, ready to
> build) · **Proposed** (recommendation made, wants maintainer confirmation) ·
> **Open** (genuinely undecided, needs a decision) · **Deferred** (out of the
> first pass).

## Why a rewrite

Hot Sheet 1 is a mature, feature-rich Node/TypeScript app (188 requirement
docs, PGLite storage, a custom JSX runtime, a Tauri shell, a deep Claude-Code
integration that grew per-tool branches everywhere). It works, but the
architecture has accreted:

- **Storage** is an embedded Postgres (PGLite/WASM) — a single opaque binary
  blob per project, hard to inspect, diff, share selectively, or put under the
  same version control the rest of a project already uses.
- **Client and service are entangled** — server-rendered HTML + a bespoke
  client runtime, one Node process doing everything.
- **AI-tool support is Claude-first** with other tools bolted on through
  scattered `if (tool === …)` branches (docs/132 in HS1 is the ongoing effort
  to undo exactly this).
- **Ticket numbers are a central Postgres sequence** — linear, requires a
  writer, conflicts under distribution/offline use.

Hot Sheet 2 keeps the *product* (a fast, local, ticket-driven worklist that
drives AI coding tools) and rebuilds the *foundation*.

## The core bets

1. **Ticketing is pluggable; git files are the default provider** — the shipped
   file-per-ticket engine remains human-readable, diffable, and versioned, while
   teams can connect GitHub Issues, Jira, or another authoritative tracker directly
   without a parallel ticket repo. One project can aggregate multiple providers;
   transfers are explicit copy/move, not continuous mirroring. See
   [02-ticket-storage.md](02-ticket-storage.md) and
   [16](16-external-sync-interface.md).
2. **A shared Rust core** — one engine (parse, store, index, query, watch,
   AI-tool host) used by the two Rust binaries, the server and the CLI. Clients
   don't embed it. See [09-technology-decisions.md](09-technology-decisions.md).
3. **SQLite as a rebuildable index** — never the source of truth, always
   derivable from disk; FTS5 for fast text search. See
   [03-indexing-and-query.md](03-indexing-and-query.md).
4. **The server is a separate, always-on process — local included** — clients are
   pure API consumers (HTTP + WebSocket + MCP) that **never embed the core**; a
   client auto-starts a local server if none is running, and the server **outlives
   the client**. See [04-core-server-cli.md](04-core-server-cli.md) §4.3.1.
5. **AI-tool integration is entirely plugin-based** — no first-class tool.
   A general interface covers init/connect, list, trigger, permissions, and
   busy-tracking. Plugins are **external + loadable** (manifest-only data →
   subprocess/WASM for behavior), and **setup + project settings are core-owned** so
   the **CLI can prepare a project headless** — no client, no server. See
   [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.1a, §5.11 and
   [04-core-server-cli.md](04-core-server-cli.md) §4.7.
6. **Random, distributable ticket IDs** — ULID-based, no central counter,
   collision-free offline. See [02-ticket-storage.md](02-ticket-storage.md) §2.4.

## Document index

Grouped by theme. File numbers are **stable ids** (referenced by tickets + cross-links)
and don't imply reading order — read by group.

**A · Product & principles**
| Doc | Topic | Status |
|---|---|---|
| [00-vision-and-principles.md](00-vision-and-principles.md) | Product north star, scope, non-goals, principles | Decided |
| `11-hs1-feature-inventory.md` | HS1 feature survey → per-area build/skip decisions, including the final search/history/command/title-shorthand carryover pass | Done (HS2-22, HS2-RFMZ1A) |

**B · Architecture & decisions**
| Doc | Topic | Status |
|---|---|---|
| [01-architecture.md](01-architecture.md) | Component map: core / server / CLI / clients / plugins | Decided |
| [09-technology-decisions.md](09-technology-decisions.md) | Consolidated ADR-style decision log (the "what + why") | Decided |

**C · Data & storage**
| Doc | Topic | Status |
|---|---|---|
| [02-ticket-storage.md](02-ticket-storage.md) | Default git-provider format, activity history, durable attachment metadata, multi-store IDs, auto-merge, copy/move | Shipped core/server/web |
| [03-indexing-and-query.md](03-indexing-and-query.md) | SQLite+FTS5 index, file watching, reindex, query surface | Confirmed |
| [17-ticket-file-format.md](17-ticket-file-format.md) | Canonical frontmatter plus bounded notes with created/edited timestamps (parser SSOT; legacy reader retained) | Shipped |

**D · Services & clients**
| Doc | Topic | Status |
|---|---|---|
| [04-core-server-cli.md](04-core-server-cli.md) | Shared core, server (+lifecycle), CLI, MCP, merge driver | Decided |
| [06-clients.md](06-clients.md) | Browser web UX → Tauri → SwiftUI macOS → iOS → Android; root real-project AppShell and local onboarding now usable; broader client floor remains | Partial |
| [ux-components.md](ux-components.md) | Cross-client component architecture and real-component `/ux-demo` catalog | Initial inventory |
| [design-guidelines.md](design-guidelines.md) | Apple macOS HIG (Tahoe / Liquid Glass) distilled for the native macOS app and the web/Tauri client: applicability matrix, rules per foundation/pattern/component/input/technology, shortcut register, menu-bar spec, AI-content rules, checklists | Decided (HS2-ZC24BS) |
| [18-dev-review-tool.md](18-dev-review-tool.md) | Embeddable Option/Alt-drag screenshot annotation and Hot Sheet ticket submission tool | Shipped initial web implementation |

**E · AI-tool integration**
| Doc | Topic | Status |
|---|---|---|
| [05-ai-tool-plugins.md](05-ai-tool-plugins.md) | Plugin interface overview (capabilities), terminals, permissions, testability | Decided |
| [13-drive-transport-interface.md](13-drive-transport-interface.md) | Drive/transport capability (capability-based, all tools) | Design |
| [14-metrics-interface.md](14-metrics-interface.md) | Usage/cost metrics capability + JSONL/rollup storage | Partial (ticketing::metrics: UsageEvent + raw JSONL writer/reader + per-contributor rollup files (`metrics/rollups/<git-email>/rollup.json`) + summary + `hotsheet-cli metrics` — HS2-69; plugin `[metrics]` mappers wired and live-verified for Codex 0.148 + Claude 2.1.241 — HS2-TJ8FGR/HS2-8PSAFE/HS2-CQ6B96. Git-sharing/team delivery → HS2-8BCRHS) |
| [15-activity-narration-interface.md](15-activity-narration-interface.md) | Activity/narration capability (for the Announcer) | Partial (HS2-KP31ZE/4C68Y8/SW655F: event model, bounded timeline, APIs, persistence+broadcast, coarse turn/permission events, and rich version-pinned Codex/Claude native activity mapped through the attributed live sink. Durable AI note policy is HS2-3GRNZW; Announcer UI/TTS is HS2-17) |

**F · Collaboration & distribution**
| Doc | Topic | Status |
|---|---|---|
| [10-assignment-and-collaboration.md](10-assignment-and-collaboration.md) | Human assignment + in-the-loop/review (vs machine claim) | Partial (data model + write path + CLI; derived views/delivery deferred) |
| [08-distributed-and-remote.md](08-distributed-and-remote.md) | Multi-server orchestration, mobile, git-native claim/lease | Design |

**G · Integrations**
| Doc | Topic | Status |
|---|---|---|
| [16-external-sync-interface.md](16-external-sync-interface.md) | Pluggable authoritative ticket providers (git default; GitHub/GitLab/Jira direct) | Shipped (HS2-ZVZP80/A90JRH/JAXS4Z/0RK4YC/VFXFFP; GitHub live CRUD validated) |

**H · Test evidence**

| Doc | Topic | Status |
|---|---|---|
| [TEST-COVERAGE.md](TEST-COVERAGE.md) | CI-validated feature matrix pairing unit, E2E, and manual evidence | Active |
| [manual-test-plan.md](manual-test-plan.md) | Checks that still require real devices or unbuilt client surfaces | Active |

**H · Engineering**
| Doc | Topic | Status |
|---|---|---|
| [12-code-organization-and-testing.md](12-code-organization-and-testing.md) | Cargo workspace/crate map, conventions, test strategy | Decided |
| [07-migration.md](07-migration.md) | PGLite → git-repo migration (standalone bundled tool) | Partial (exporter + importer built and validated against real clusters/project snapshot; deterministic HS2 ids, normalized close state, no retained HS1 fields; UI flow pending) |

> **Core decisions confirmed by the maintainer 2026-08-19:** Rust core · git-file
> default provider · ULID + **all-caps** slug · SQLite+FTS5 · **automatic conflict
> resolution** (semantic merge driver) · **aggressive automatic repo sync** ·
> **shared-vs-local data on-disk (gitignored), index is only a cache** · inline
> notes with timestamp-ordered UUIDs · attachments · **server always a separate
> process (local included), client auto-starts it and it outlives the client;
> clients never embed the core** · client order browser web UX → Tauri host →
> SwiftUI macOS → iOS → Android · plugin-only AI tools.
>
> **Round-2 confirmations (2026-08-19, §9.11):** one server per machine · per-project
> MCP shim · close-reason open/closed axis · id-prefix sharding · PTY sizing
> focus-follows · assignment = git email + committed `people.json` + one control +
> soft review · orchestration **live-mount only** (no auto-clone) · multi-machine =
> **git-native self-claim** (ref/tag CAS, no coordinator) · UI = **Kerf (`kerfjs`) + Web Awesome Core** (validated custom-element stack; see 06 §6.3 / 09 §9.5) · deferred
> past v1: cross-server views, iOS push, remote terminals, iOS local stores. The HS1
> feature inventory is complete (doc 11); only implementation-time details remain open.
>
> **Round-3 confirmation (2026-08-20):** AI-tool **setup/instructions/skills/MCP and
> project settings are core-owned**, driven by **either the CLI (headless — no
> client, no server) or the server**, not the app layer (reverses HS1). Plugins are
> **external + loadable**: manifest-only (data, no ABI) for the bulk, **subprocess**
> for process-shaped behaviors (drive/terminal/MCP) and **WASM** for pure-compute,
> built-ins through the same loader, with a trust gate + `hotsheet plugin verify`.
> Settings split **shared (committed) / local (gitignored, machine) / client-only
> (device)** — core owns the first two. See [04](04-core-server-cli.md) §4.1/§4.7,
> [05](05-ai-tool-plugins.md) §5.1a/§5.11.
>
> **Ticket-provider revision (2026-08-26, HS2-QJ5TCT):** ticketing is pluggable;
> the git-file engine is the default provider, while GitHub Issues/Jira/GitLab are
> accessed directly as authoritative systems. A code project may connect multiple
> providers. There is no continuous cross-provider mirroring; explicit copy/move is
> idempotent and retains source provenance.

## Requirements summary (synthesized status view)

Keep this current as the design firms up. Statuses: **Design** (specified here) ·
**Confirm** (needs maintainer sign-off) · **Deferred**.

| Capability | Where | Status |
|---|---|---|
| Git-repo file-per-ticket storage | 02, 17 | Confirmed; **parser + FsStore built** (`hotsheet-model::format`, `hotsheet-ticketing::store`) |
| Persisted-format and unsynchronized-rollout compatibility | 19 | Shipped pre-release policy/guards: explicit format activation, retained ticket/store/project/settings fixtures, upgrade-required classification for newer formats, and protocol-range gating before project API use; first public release freezes permanent backward-readability fixtures |
| Pluggable authoritative ticket providers (git default; direct GitHub/Jira/GitLab) | 16 | Shipped: provider contract/capabilities/qualified wire identity/config registry + git adapter + CLI/server/MCP discovery/routes (HS2-ZVZP80); idempotent transfer (HS2-A90JRH); direct GitHub with real credential-gated CRUD validation (HS2-JAXS4Z); GitLab/Jira adapters (HS2-0RK4YC); Kerf client connection/default/filter/capability/copy-move UX (HS2-VFXFFP) |
| Standalone foreground server launch | 04 §4.3 | Shipped: `hotsheet-cli serve` resolves a sibling/PATH server, rejects version drift, and forwards start/stop; detached supervision remains client-owned |
| Multiple ticket stores per project (mixed permissions/locality) | 02 | Confirmed |
| Every **git-provider store** is a git repo (local-only = no remote) | 02 §2.1 | Confirmed |
| Store identity/naming (id + name + per-store prefix); positional membership | 02 §2.2.1 | Confirmed (design) |
| Copy/move tickets between stores (move = copy + source tombstone) | 02 §2.13 | Shipped, minus UI (core `ops::copy_ticket`/`move_ticket` + `hotsheet copy`/`move` CLI — HS2-60; `hotsheet_copy`/`hotsheet_move` MCP tools + `POST /tickets/{id}/copy\|move` server endpoints + `copied_from`/`moved_to_store` on the wire + cross-store index resolve via `StoreHost::resolve`/`open_only` — HS2-S4H2AM. Remaining: the drag-onto-store / "Move to store…" **client** affordance) |
| ULID-based ticket IDs, all-caps slug (no central sequence) | 02 §2.4 | Confirmed |
| **Automatic conflict resolution (semantic merge driver)** | 02 §2.7 | Shipped (`hotsheet merge-driver` field-by-field 3-way merge: scalars LWW-by-`updated_at`, tags/blocked_by/assignees set-union, notes union-by-ULID, body text-merged only when both sides change it; registered by `init` via `.gitattributes`+git config; `doctor` flags missing registration; proptests + real-git E2E; HS2-18) |
| Inline notes with timestamp-ordered UUIDs | 02 §2.6 | Confirmed |
| Attachments support | 02 §2.5 | Confirmed |
| **Automatic repo sync (aggressive fetch/push/rebase)** | 02 §2.12 | Shipped (`ticketing::sync_once`: fetch → rebase-through-merge-driver → push, offline-tolerant, conflict-aborts-clean; `hotsheet sync` CLI + bare-remote E2E — HS2-19; server `sync_loop`: interval/event-driven wake, coalescing, capped backoff, every hosted store, watcher-safe content-hash dedup — HS2-731C2X) |
| **Shared vs. local ticket data (on-disk gitignored overlay)** | 02 §2.11 | Partial (field classification + `ticketing::LocalOverlay` gitignored `<store>/local/` mechanism + read-tracking (`hotsheet read`/`ls ●`) — HS2-21; feedback-draft/UI-state/pref overlay pending → HS2-AWTHJE) |
| **Human assignment + in-the-loop/review** | 10 | Core/server shipped (`assignees` + review model, roster, merge, CLI/server/MCP assignment, indexed `me` views, assignment live events, recipient notifications, GitHub public-email roster seed — HS2-20/89/T84F9F/TCDTCH/NZT80R). Native/on-sync People controls and attention presentation remain client work → HS2-CRW5CP |
| **Close reasons** (completed / not planned / duplicate-of / obsolete) | 02 §2.6a | Shipped (field + `close` op + index columns + query filter `close_reason`/`closed` across CLI/MCP/server + reopen-clears; HS2-61) |
| SQLite + FTS5 index, rebuildable from disk | 03 | Shipped (`hotsheet-index`: schema/query/FTS/rebuild + **file-backed restore/reconcile on launch** + facet tables (tags/assignees/reviews) + keyset `page_after` paging; `reindex` CLI — HS2-5/88/89/T84F9F/TCDTCH) |
| Filesystem watch → incremental reindex | 03 §3.4 | Shipped (server `spawn_watcher`: content-hash reindex + WS events; index reconcile uses a git-diff fast path on clean HEAD moves and falls back to a full hash walk for dirty/non-git stores) |
| Shared **Rust** core engine (server + CLI only; clients don't embed) | 04, 09 | Confirmed |
| Server always separate + client auto-start + outlives client | 04 §4.3.1, 09 §9.1e | Partial (server-side lifecycle shipped — `hotsheet-server::lifecycle`: instance registry + discovery, per-store index-writer lock, `serve --stop`, graceful shutdown, attach-if-already-running; E2E-verified — HS2-59. Client-side detached auto-start + supervise pending → HS2-4072GM) |
| Independent server (HTTP + WS) | 04 §4.3 | Partial (`hotsheet-server`: REST + /ws/sync + loopback auth; index/watcher/lifecycle shipped; **Tier-1 mTLS shipped** — off-loopback binds require a per-project-CA client cert, with live revocation, opt-in per-fingerprint read-only/read-write/deny ACLs, and explicit 90-day device renewal, HS2-VT3JMF/MPC0QF) |
| One server per machine serves all projects (multi-store) | 04 §4.3.1 | Partial (`server::multistore`: StoreHost + GET/POST /stores + scoped /stores/{id}/tickets read+write + GET /resolve/{ulid} cross-store + per-store watcher + file-backed indexes + stores.json discovery + one-machine-server-per-project discovery reconciliation (topology A) — HS2-87; **per-store index-writer locks** for every hosted store — HS2-AYCA1W. Remaining: a client to drive multi-project discovery/join → HS2-4072GM) |
| MCP `hotsheet_*` tools (per-project shim) | 05 §5.8 | Shipped (`hotsheet-mcp`: **serverless direct-to-disk (`--path`) or server-proxy (`--server`)**; core-owned `setup` writes each plugin's MCP configuration and conformance-tests it; HTTP auth failures probe `/health`'s HS2 generation/store marker to distinguish bad secrets from HS1/wrong endpoints — HS2-8H8BQM) |
| Direct-to-disk CLI (+ `merge-driver`) | 04 §4.4 | Shipped (`hotsheet` init/link/new/ls/show/edit/close/copy/move/assign/people/read/setup/plugin/settings/key/import/sync/merge-driver/doctor/reindex/worklist/metrics/serve/cert/claim/claim-next/release/renew/trigger/work/launch/permission-hook`; `launch claude` reuses the checkout's local store link and running-server instance to route an ordinary external-terminal Claude session's permissions; `key` manages OS-keychain provider credentials without argv/settings values; `init --standalone [--at/--remote]` creates, git-initializes, configures, and links the recommended separate store, then prints read-only installed-tool/HS1 guidance also available from `doctor`) |
| **Headless AI-tool loop** (setup → skills/CLI/MCP, with/without server) | 04 §4.4, 05 §5.1a/§5.8/§5.11 | Shipped (`setup` + serverless/server MCP, permission-protocol emulator, version-pinned protocol cassettes/drift oracles, and opt-in live tiers — HS2-95/1GJY50/SSHCRM/PEQ6Q8/CQ6B96) |
| Plugin-only AI-tool integration (host + registry + capability accessors + first plugins) | 05 | Built (HS2-9/PEQ6Q8: plugin registry and conformance gate; Claude+Codex+Antigravity+OpenCode bundled; OpenCode has merge-safe MCP setup plus a live ACP stdio session client, streaming/cancel, metrics, and drift oracle; plugin-first lint forbids core branching on a tool id) |
| **Core-owned AI-tool setup/instructions/skills/MCP (headless; CLI + server, not app)** | 05 §5.1a, 04 §4.1 | Built (HS2-91): `hotsheet_plugins::run_setup` is the one impl — CLI `setup <tool>`/`--detect` (headless) + server `POST /setup/<tool>` both call it; managed instruction block + skill + per-tool MCP-config writers; server E2E confirms identical result) |
| Cross-tool activity stream | 15 | Core + rich native drive stream shipped: rolling digest storage, timeline API, WebSocket/long-poll payloads, coarse turn/permission events, and attributed Codex 0.152.1 completed-item / Claude 2.1.258 tool-use activity (HS2-KP31ZE/4C68Y8/SW655F). Durable AI note policy is HS2-3GRNZW; Announcer UI/TTS remains post-floor |
| **External loadable plugins (manifest-only → subprocess/WASM; trust gate + verify)** | 05 §5.11 | Partial (HS2-92 loader + `setup <third-party>`; HS2-93 trust gate: `plugin verify`/`info`/`install` disclosure + path-safety; subprocess/WASM sandbox still to come) |
| **AI-tool testing harness: conformance gate + hs-fake-agent** | 12 §12.7.7 | Shipped (HS2-64/1GJY50/SSHCRM: registry-parameterized conformance, deterministic PTY-byte and permission-protocol emulators, integrated terminal E2E, and versioned protocol cassette/drift coverage; live tiers remain opt-in by design) |
| **Project settings, core-owned (global/shared/local scopes, CLI-manageable; client owns device-only)** | 04 §4.7 | Built (HS2-94/34): `Settings` global/shared/local + `hotsheet-cli settings get/set/list`; local auto-gitignored |
| Auto-context by category/tag | 05 §5.9 | Shipped (HS2-BZBVAS): HS1-compatible defaults + layered overrides/suppression; computed worklist and REST/MCP get/query/claim guidance |
| Secure provider/API-key registry | 04 §4.7.1 | Shipped (HS2-M1XMSX): injected secret-store port + macOS Keychain/Linux Secret Service adapters, metadata-only global registry, stdin-only CLI writes, env-only fallback |
| Checkout identity/discovery | 04 §4.3 | Shipped core/server/CLI/MCP (HS2-NGC8AE/VSPFD9): readable canonical-path ids, many-to-many store links, setup registration, authenticated discovery and checkout-qualified ticket CRUD; server bearer tokens remain separate |
| Headless repository/dashboard APIs | 04 §4.3 | Shipped server-testable slices (HS2-RPVFA4/38RJMK): git status snapshot, current flow/throughput/cycle time, and usage summary; client visualizations remain |
| Safe configured commands | 04 §4.3 | Shipped headless slice (HS2-JN3X4W): typed argv settings, configured-id-only execution, output cursors, cancellation, bounded history; UI remains |
| Notifications and speech | 04 §4.3 | Shipped server boundaries (HS2-ZP869N/5PSQJQ): routed/deduplicated notifications plus server-owned injectable TTS providers; native presentation remains |
| Terminal/PTY hosting for AI tools | 05 §5.4 | Shipped (`hotsheet-terminals`: PTY + scrollback/live fan-out, manager, OSC 7/8/9/133 + spinner busy inference, server CRUD/input + live WS attach, tool-terminal busy→registry feed, explicit plugin `[launch]` setup/command composition, fake-agent permission round-trip E2E, and detached broker preserving terminal streams/sizing across server restart with Ping/Pong health + conservative idle-GC/socket cleanup — HS2-10/A6R5QV/XTTTMV/RCKEJ9/4M67VN/G0ETNQ/SSHCRM/8HHFHN/ERT00F/SV3XS8) |
| Multi-viewer PTY sizing (server-arbitrated, focus-follows, leased; remote-safe) | 06 §6.7 | Partial (HS2-BD7Q74: `hotsheet-terminals::SizeArbiter` — focus-follows default + smallest/largest/pinned, leased viewport claims + heartbeat/expire + focus-hold/min-delta/min-interval guards + disconnect self-heal, pure & transition/adversarial-tested; wired into the WS attach — size claims in, `{pty_size,driven_by}` out. HS2-946EQG settled the client grid interaction contract, including width-based 1–10 and short-container height-based 1–3 scale; implementation is HS2-2ZCN7K) |
| Drive/transport interface (steer a running tool) | 13, 05 §5.5 | Shipped (`Drive`/`SpawnDrive`, persistent app-server/channel/ACP drives, async `TurnEvent`, trigger/work loops, and version-pinned live protocol verification/drift oracles — HS2-106/108/115/DTPX2V/1TY7GC/PEQ6Q8/CQ6B96) |
| Connection registry + busy tracking | 05 §5.6 | Shipped (`ConnectionRegistry` + sliding-window busy; live turns heartbeat it; hosted PTYs register and feed OSC-133/spinner inference — HS2-107/34X6BW/4M67VN) |
| Trigger / permissions bridge | 05 §5.5, §5.7; 06 §6.6 | Shipped (`hotsheet-cli trigger`/`work`; capability-aware external-terminal `launch` with Claude hook route-back and no-`-C` linked-store discovery; FIFO `SharedPermissionBridge`; machine-local durable Always Allow rules; shared 24-hour eventual safe-deny guard; server `/permissions`,`/permissions/{id}`,`/permissions/ask`; Codex app-server approvals + Claude PreToolUse hook; web popup, cross-project badges, Notifications history, Ignore, and visible-only auto-Allow/auto-Deny — HS2-11/HS2-9R9YZW/HS2-YMR9HE/HS2-XCTAHM/HS2-TFBV7Z/HS2-C46G58) |
| Browser web UX client (1st) → Tauri host (2nd) | 06, UX catalog | Partial; the production shell and `/ux-demo` cover the main ticket workflow. Remaining feature-floor work is explicitly tracked: shell/navigation (HS2-W6JHT1), advanced search (HS2-383D6K), persistent selection/undo (HS2-4CAN74), board states (HS2-0W67Y6), global drops (HS2-R6P8MZ), catalog tooling (HS2-89692E), people/review UX (HS2-CRW5CP), settings (HS2-BDW1BN/S4TZ31), and native host/lifecycle work |
| Native SwiftUI macOS (3rd) → iOS (4th) | 06, UX catalog | Confirmed; macOS follows the same conceptual component responsibilities using native SwiftUI primitives |
| **Apple HIG adoption (macOS fully; web/Tauri "HIG-shaped, not HIG-skinned")** | design-guidelines | Decided (HS2-ZC24BS): guidelines doc shipped; web gaps tracked — system appearance/dark mode (HS2-DKZG9S), typography (HS2-PS29TA), native icon decision (HS2-0P83KD), menu bar/command registry (HS2-80VPPW), conformance audit (HS2-A2A9GT), AI-content labeling (HS2-WBW3Z9), iOS considerations (HS2-46RA38) |
| Android client (5th, Kotlin/Compose) | 06 | Deferred (sequence-confirmed) |
| Code organization (Cargo workspace/crates) + test strategy | 12 | Decided |
| PGLite → git migration (UI-prompted) | 07 | Partial (Node exporter + Rust importer + conformance test; deterministic/idempotent HS2 identity and normalized close state shipped; UI flow deferred) |
| Multi-server orchestration (live-mount only; no auto-clone) | 08 §8.2 | Confirmed (design) |
| Git-native multi-machine claim/lease (ref/tag CAS, no coordinator) | 08 §8.5 | Shipped (`ticketing::distclaim`: `refs/hotsheet/claims/<ulid>` push-CAS claim [first-wins], `--force-with-lease` renew/steal, `ls-remote` enumerate, expiry sweep — bare-remote E2E; HS2-84. `distwork::select_and_claim`/`work_once` self-claim cycle [HS2-E7RXXR]; `server::dist_work_loop` drives a real AI tool per claimed ticket [`--drive-tool`, off by default, HS2-DTPX2V/HS2-1TY7GC]) |
| Mobile ↔ server configuration/pairing (mTLS + QR) | 08 §8.3 | Partial (the mTLS core is built — per-project CA + expiring/renewable device certs + live revocation + per-device ACLs + off-loopback serving, HS2-VT3JMF/MPC0QF; `.p12`/QR enrollment is tracked by HS2-KQ8NEP) |

## Conventions

- American English throughout (prose, comments, UI strings).
- Cross-reference related docs with relative links.
- **Diagrams are rendered, not ASCII art** — Mermaid fenced blocks for
  flow/sequence/graph diagrams, or SVG (authored as HTML, rendered with
  `domotion`) for designed layout diagrams. See [diagrams/README.md](diagrams/README.md).
- When a design decision changes, update the doc **and** this index in the same
  change.
- Follow-up work is tracked as Hot Sheet tickets (see the worklist), not TODOs
  buried in prose.
