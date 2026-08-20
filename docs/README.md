# Hot Sheet 2 — Design Docs

This directory holds the design for **Hot Sheet 2** — a **Small Tale Inc.** project,
and a from-scratch rewrite of the original Hot Sheet (`~/Documents/hotsheet`). It is
**design only** — no implementation exists yet. These documents are the source of
truth for what we are going to build and why.

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

1. **Tickets live in git repos as plain files** — one file per ticket,
   human-readable, diffable, selectively shareable, and versioned by the same
   git the user already trusts. Git is the source of truth, and merges are
   **almost entirely automatic** via a format-aware merge driver. See
   [02-ticket-storage.md](02-ticket-storage.md).
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
| `11-hs1-feature-inventory.md` | HS1 feature survey → per-area build/skip decisions (reference) | Done (HS2-22) |

**B · Architecture & decisions**
| Doc | Topic | Status |
|---|---|---|
| [01-architecture.md](01-architecture.md) | Component map: core / server / CLI / clients / plugins | Decided |
| [09-technology-decisions.md](09-technology-decisions.md) | Consolidated ADR-style decision log (the "what + why") | Decided |

**C · Data & storage**
| Doc | Topic | Status |
|---|---|---|
| [02-ticket-storage.md](02-ticket-storage.md) | Git-repo ticket format, multi-store projects, IDs, auto-merge, copy/move | Confirmed |
| [03-indexing-and-query.md](03-indexing-and-query.md) | SQLite+FTS5 index, file watching, reindex, query surface | Confirmed |
| [17-ticket-file-format.md](17-ticket-file-format.md) | Canonical frontmatter/notes field schema (parser SSOT) | Decided |

**D · Services & clients**
| Doc | Topic | Status |
|---|---|---|
| [04-core-server-cli.md](04-core-server-cli.md) | Shared core, server (+lifecycle), CLI, MCP, merge driver | Decided |
| [06-clients.md](06-clients.md) | Tauri+web → SwiftUI macOS → iOS → Android; terminal sizing; notes/reader | Decided |

**E · AI-tool integration**
| Doc | Topic | Status |
|---|---|---|
| [05-ai-tool-plugins.md](05-ai-tool-plugins.md) | Plugin interface overview (capabilities), terminals, permissions, testability | Decided |
| [13-drive-transport-interface.md](13-drive-transport-interface.md) | Drive/transport capability (capability-based, all tools) | Design |
| [14-metrics-interface.md](14-metrics-interface.md) | Usage/cost metrics capability + JSONL/rollup storage | Design |
| [15-activity-narration-interface.md](15-activity-narration-interface.md) | Activity/narration capability (for the Announcer) | Design |

**F · Collaboration & distribution**
| Doc | Topic | Status |
|---|---|---|
| [10-assignment-and-collaboration.md](10-assignment-and-collaboration.md) | Human assignment + in-the-loop/review (vs machine claim) | Design |
| [08-distributed-and-remote.md](08-distributed-and-remote.md) | Multi-server orchestration, mobile, git-native claim/lease | Design |

**G · Integrations**
| Doc | Topic | Status |
|---|---|---|
| [16-external-sync-interface.md](16-external-sync-interface.md) | External-sync plugin interface (GitHub/GitLab/Jira) | Design |

**H · Engineering**
| Doc | Topic | Status |
|---|---|---|
| [12-code-organization-and-testing.md](12-code-organization-and-testing.md) | Cargo workspace/crate map, conventions, test strategy | Decided |
| [07-migration.md](07-migration.md) | PGLite → git-repo migration (standalone bundled tool) | Partial (exporter + importer built; UI flow + real-cluster run pending) |

> **Core decisions confirmed by the maintainer 2026-08-19:** Rust core · git-file
> storage · ULID + **all-caps** slug · SQLite+FTS5 · **automatic conflict
> resolution** (semantic merge driver) · **aggressive automatic repo sync** ·
> **shared-vs-local data on-disk (gitignored), index is only a cache** · inline
> notes with timestamp-ordered UUIDs · attachments · **server always a separate
> process (local included), client auto-starts it and it outlives the client;
> clients never embed the core** · client order Tauri+web → SwiftUI macOS → iOS →
> Android · plugin-only AI tools.
>
> **Round-2 confirmations (2026-08-19, §9.11):** one server per machine · per-project
> MCP shim · close-reason open/closed axis · id-prefix sharding · PTY sizing
> focus-follows · assignment = git email + committed `people.json` + one control +
> soft review · orchestration **live-mount only** (no auto-clone) · multi-machine =
> **git-native self-claim** (ref/tag CAS, no coordinator) · UI = **Solid** · deferred
> past v1: cross-server views, iOS push, remote terminals, iOS local stores. Remaining
> open: the HS1 feature inventory (doc 11) + small implementation-time details.
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

## Requirements summary (synthesized status view)

Keep this current as the design firms up. Statuses: **Design** (specified here) ·
**Confirm** (needs maintainer sign-off) · **Deferred**.

| Capability | Where | Status |
|---|---|---|
| Git-repo file-per-ticket storage | 02, 17 | Confirmed; **parser + FsStore built** (`hotsheet-model::format`, `hotsheet-ticketing::store`) |
| Multiple ticket stores per project (mixed permissions/locality) | 02 | Confirmed |
| Every store is a git repo (local-only = no remote) | 02 §2.1 | Confirmed |
| Store identity/naming (id + name + per-store prefix); positional membership | 02 §2.2.1 | Confirmed (design) |
| Copy/move tickets between stores (move = copy + source tombstone) | 02 §2.13 | Confirmed (design) |
| ULID-based ticket IDs, all-caps slug (no central sequence) | 02 §2.4 | Confirmed |
| **Automatic conflict resolution (semantic merge driver)** | 02 §2.7 | Confirmed |
| Inline notes with timestamp-ordered UUIDs | 02 §2.6 | Confirmed |
| Attachments support | 02 §2.5 | Confirmed |
| **Automatic repo sync (aggressive fetch/push/rebase)** | 02 §2.12 | Confirmed (design) |
| **Shared vs. local ticket data (on-disk gitignored overlay)** | 02 §2.11 | Confirmed (design) |
| **Human assignment + in-the-loop/review** | 10 | Open (proposal) |
| **Close reasons** (completed / not planned / duplicate-of / obsolete) | 02 §2.6a | Confirmed (field); status coupling → HS2-24 |
| SQLite + FTS5 index, rebuildable from disk | 03 | Partial (`hotsheet-index`: schema/query/FTS/rebuild + **file-backed restore/reconcile on launch** built; facet tables + paging + `reindex` CLI pending) |
| Filesystem watch → incremental reindex | 03 §3.4 | Partial (server `spawn_watcher`: content-hash reindex + WS events built; git-diff fast path pending) |
| Shared **Rust** core engine (server + CLI only; clients don't embed) | 04, 09 | Confirmed |
| Server always separate + client auto-start + outlives client | 04 §4.3.1, 09 §9.1e | Confirmed (design) |
| Independent server (HTTP + WS) | 04 §4.3 | Partial (`hotsheet-server`: REST + /ws/sync + loopback auth; index/watcher/lifecycle/mTLS pending) |
| MCP `hotsheet_*` tools (per-project shim) | 05 §5.8 | Partial (`hotsheet-mcp` shim: **serverless direct-to-disk (`--path`) or server-proxy (`--server`)**; config-writing pending) |
| Direct-to-disk CLI (+ `merge-driver`) | 04 §4.4 | Partial (`hotsheet` init/new/ls/show/edit/close/**setup**/import; merge-driver + more pending) |
| **Headless AI-tool loop** (setup → skills/CLI/MCP, with/without server) | 04 §4.4, 05 §5.1a/§5.8/§5.11 | Partial (`setup claude` + serverless/server MCP built; E2E harness `test-projects/e2e-headless-claude.sh` passes both modes; live-Claude tier opt-in — HS2-95) |
| Plugin-only AI-tool integration | 05 | Confirmed (design) |
| **Core-owned AI-tool setup/instructions/skills/MCP (headless; CLI + server, not app)** | 05 §5.1a, 04 §4.1 | Confirmed (design); build HS2-91 |
| **External loadable plugins (manifest-only → subprocess/WASM; trust gate + verify)** | 05 §5.11 | Confirmed (design); build HS2-92/HS2-93 |
| **Project settings, core-owned (shared/local scopes, CLI-manageable; client owns device-only)** | 04 §4.7 | Confirmed (design); build HS2-94 |
| Terminal/PTY hosting for AI tools | 05 §5.4 | Design |
| Multi-viewer PTY sizing (server-arbitrated, focus-follows, leased; remote-safe) | 06 §6.7 | Design |
| Connection registry / trigger / permissions / busy | 05 | Design |
| Tauri + web client (1st) | 06 | Confirmed |
| Native SwiftUI macOS (2nd) → iOS (3rd) | 06 | Confirmed |
| Android client (4th, Kotlin/Compose) | 06 | Deferred (sequence-confirmed) |
| Code organization (Cargo workspace/crates) + test strategy | 12 | Decided |
| PGLite → git migration (UI-prompted) | 07 | Partial (Node exporter + Rust importer + conformance test; UI flow deferred) |
| Multi-server orchestration (live-mount only; no auto-clone) | 08 §8.2 | Confirmed (design) |
| Git-native multi-machine claim/lease (ref/tag CAS, no coordinator) | 08 §8.5 | Confirmed (design); spike HS2-63 |
| Mobile ↔ server configuration/pairing (mTLS + QR) | 08 §8.3 | Design |

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
