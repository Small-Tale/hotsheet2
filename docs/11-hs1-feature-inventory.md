# 11. HS1 Feature Inventory → Rewrite Decisions

> **Status: In progress (HS2-22).** A thorough survey of every high- and low-level
> feature in Hot Sheet 1 (`~/Documents/hotsheet`, README + 188 requirement docs),
> so each can get an explicit **"build it in Hot Sheet 2? and if so, with what
> changes?"** decision — the maintainer's ask (2026-08-19).

## How to use this doc

- Features are grouped into **36 areas**. Doc numbers refer to HS1 `docs/NN`.
- Each area has a **recommended disposition** (a starting hint, not a ruling):
  - **core-keep** — clearly belongs; port it.
  - **keep-with-changes** — port, but rethink how it works/operates.
  - **reconsider** — real question whether it earns its place; decide deliberately.
  - **likely-drop** — probably don't rebuild (retired, superseded, or low value).
- Each area gets a **decision ticket** (`DECIDE: <area>`) posing port/change/drop for
  that area with its sub-features enumerated. Those tickets are children of the
  survey epic **HS2-22**. Areas already covered by a concrete build ticket
  (HS2-3…HS2-14) are noted so we don't double-track.

The single best HS1 companion when scoping any area: `docs/ai/requirements-summary.md`
in HS1 (a maintained, status-marked synthesis of every requirement doc).

---

## 1. Ticket Model & CRUD — **core-keep** (build: HS2-3/4)
Foundational entity + lifecycle. Number/title/details/category/priority/status/
up_next/tags/notes/attachments/timestamps (docs/3); soft-delete + restore; duplicate
(docs/3, 9); flat blocked-by gate (docs/3, 90, 116); blocked-reason free text
(docs/116); unread/blue-dot + mark read (docs/3, 4); REST-as-truth + validation
(docs/9). *Change for HS2:* ULID ids + all-caps slug, git-file backing (docs/02).

## 2. Statuses, Priorities & Categories — **core-keep** (consider trimming statuses)
7 statuses + transition side effects (docs/3); 5 priorities (docs/3); customizable
categories w/ color/badge/shortcut + presets (docs/3); custom prefix (docs/3);
auto-cleanup verified→archive / trash→delete (docs/3). **DECIDED (HS2-24,
2026-08-19): keep HS1's status set unchanged** (no trim, no open/closed collapse,
no new `closed` status) + add a **separate optional `close_reason`** field
(`completed`/`not_planned`/`duplicate`+`duplicate_of`/`obsolete`) for tracking why a
ticket was closed (docs/02 §2.6a, HS2-61).

## 3. Tags & Auto-Context — **core-keep (both)** (DECIDED, HS2-25, 2026-08-19)
Tags: normalization, autocomplete, batch dialog, chips, view-associated auto-tag
(docs/3, 4) — keep. **Auto-context** (per-category/tag AI guidance injected into the
worklist) (docs/4 §4.18) — **keep; it's a critical v1 feature** (maintainer). An
AI-facing concept carried over — see docs/05 §5.9.

## 4. Notes, Reader Mode & Feedback — **core-keep, with improvements** (DECIDED, HS2-26, 2026-08-19)
Inline-editable notes (docs/3); **FEEDBACK NEEDED / IMMEDIATE** prefixes + feedback
dialog w/ draft (docs/21) — core to the AI loop, and generalizes to human assignment
([10](10-assignment-and-collaboration.md)). **Improvements (maintainer):** four note
**kinds** (regular / feedback_needed / feedback_draft / status — docs/02 §2.6); **one
reader mode** whose rendering is driven by note kind, not launch point (feedback
kinds → editor style; regular/status/details → reader style); and an **"Edit" button
→ larger editing surface**, with the reader button reachable *while* editing to jump
straight into it. Design in docs/06 §6.8; build HS2-65.

## 5. Attachments — **core-keep** (build: HS2-4)
File picker + drag-drop (docs/5); reveal in file manager; copy across paste/project
(docs/3, 5); paste from clipboard (docs/77); promised-file drops (docs/130). *Change:*
attachments are files under `attachments/<id>/` in the store (docs/02 §2.5);
backups/GC (docs/43) obsoleted by git.

## 6. Up Next, Worklist & Markdown Sync — **core-keep** (the product's core)
Up Next star + status auto-reset (docs/3); `worklist.md` + `open-tickets.md`
auto-generated from the index (docs/6). *Change:* generated from SQLite index
(docs/03 §3.6), not the DB.

## 7. Views & Layouts — **core-keep + expand the query builder** (DECIDED, HS2-29, 2026-08-19)
List w/ pagination, column/kanban drag, custom views + query builder (docs/4 §4.14),
sidebar built-in views + counts, sort, detail-panel orientation, draft row, FLIP
animations (docs/4). **DECIDED (maintainer): keep the query builder AND expand it to
the new HS2 fields** — `close_reason`, **ticket repo / store**, and **user
assignment** (assignees / review-requests). The index + query surface already carry
these dimensions (docs/03 §3.3/§3.5). Per-machine custom views (docs/107) → the
shared/local model (docs/02 §2.11).

## 8. Search & Filter — **core-keep** (build: HS2-5)
FTS over title/details/number/tags/notes, multi-word union, exact-id lookup,
category/priority filters, include-backlog/archive rows (docs/4 §4.8, 40). *Change:*
FTS5 over the index (docs/03).

## 9. Batch Operations & Context Menus — **core-keep**
Multi-select batch edits, overflow menu, full right-click menu, move-to
backlog/open, batch re-open (docs/3, 4, 66). Straight port.

## 10. Undo/Redo, Clipboard & Drag-Drop — **core-keep**
Undo/redo across all ops (docs/4 §4.21); copy/cut/paste tickets w/ dedup (docs/4
§4.12); drag-to-sidebar to set fields; keyboard shortcuts (docs/4). Straight port
(client-side).

## 11. Multi-Project Tabs & Cross-Project — **core-keep**
Multi-project tabs w/ per-project remembered state (docs/4 §4.2); cross-project
ticket drag (docs/76); cross-project bell (docs/24); cross-project stats (docs/70);
project-scoped client-state guard (docs/125, 126). *Change:* a "project" now
references one-or-more git stores (docs/02 §2.2).

## 12. Settings & Sharing Layers — **keep-with-changes**
Portable `settings.json`; Shared|Local|Resolved scope control + classification
(docs/95, 2); many settings tabs (docs/4 §4.13); In-Development gates (docs/124).
*Change:* maps directly onto the shared-vs-local data model (docs/02 §2.11); simplify
the layer UI if possible.

## 13. Embedded Terminals — **core-keep** (build: HS2-10)
Per-project persistent PTYs, multiple/dynamic terminals, themes/fonts, OSC titles +
bell, find widget, quit-confirm, global checkout, multi-client, **PTY broker
survival** (docs/22, 23, 34, 35, 37, 51, 54, 109, 136). Signature feature — port the
broker + busy inference.

## 14. Terminal Dashboard & Grids — **core-keep (critical, v1)** (DECIDED, HS2-36, 2026-08-19)
Global terminal dashboard, drawer grid, magnify/dedicated/jump, column slider,
visibility groupings (docs/25, 36, 38, 39, 56). **DECIDED (maintainer): keep — these
are critical, heavily-used features, not deferred.** Each tile is a terminal viewport,
so they compose with the multi-viewer PTY-sizing arbiter (docs/06 §6.7) — a
magnified/dedicated tile is just a focused viewport. Build with the terminal work
(HS2-10).

## 15. Shell Integration (OSC) — **keep 7/8/9; drop OSC 133 for now** (DECIDED, HS2-37, 2026-08-19)
**Keep:** OSC 7 (cwd chip), OSC 8 (clickable hyperlinks), OSC 9 (notifications —
in-app toasts + native OS notifications when backgrounded). **Drop for now (v1):** the
entire **OSC 133** suite — prompt markers / jump / copy-last-output / hover-to-rerun /
ask-Claude (docs/26–33). Maintainer decision. Revisit OSC 133 later if wanted.

## 16. Claude Channel & Permission Overlay — **keep-with-changes** (build: HS2-9/11)
Play button + auto mode + backoff, auto-prioritize, permission overlay w/ edit-diff,
per-project allow-rules, commands log, busy/idle (docs/12, 14, 47, 64). *Change:*
becomes **one plugin's transport** among many; the permission overlay is the generic
host bridge (docs/05 §5.7).

## 17. Custom Commands & Command Buttons — **keep-with-changes** (DECIDED, HS2-39, 2026-08-19)
Named command buttons (Claude/shell), command groups, spinner+stop, long-press,
last-run, local customization (docs/15, 16, 57, 83, 84, 108). **DECIDED (maintainer):
port these; DROP the retired worker target-picker** (docs/103's Main/Worker/All
UI — it existed only for the removed worker pool). The drive-level `target` (docs/05
§5.5, picks which live connection a trigger hits) is unrelated and stays.

## 18. AI-Tool Multi-Tool Support & Plugin Registry — **core-keep as the model** (DECIDED, HS2-40, 2026-08-19)
The multi-tool epic + capability table + **plugin interface** + availability/opt-in +
adapter config generation (docs/113, 117, 132, 133, 118, 119, 120) — the starting
point for docs/05. **DECIDED (maintainer): v1 ships Claude + Codex** (both critical).
**Acceptance test for the whole testability design (HS2-64):** the maintainer should
be able to ask, *relatively unsupervised*, for support for another tool (OpenCode /
Cursor / Antigravity / …) and have it **work fully without constant manual testing** —
inherited via the fake-agent E2E + conformance gate + recorded contracts.

## 19. Drive Transports (MCP-hooks / ACP / Codex) — **reconsider** (minimal set)
MCP+hooks (docs/115), ACP (docs/114), Codex app-server/daemon/model-B (docs/121,
129). *Decide:* which transports v1 supports (Claude channel is a given; pick 1–2
more). docs/05 §5.5 models them as one trait.

## 20. Skills / Instructions Generation — **keep-with-changes**
Skill/rule files for Claude/Cursor/Copilot/Windsurf, managed CLAUDE.md/AGENTS.md
sections, self-healing test-setup block (docs/6, 86). Port as plugin capabilities
(docs/05 §5.3).

## 21. MCP Tool Surface — **core-keep** (build: HS2-7/9)
The `hotsheet_*` tools proxying the core (create/update/query/batch/claim/
announce/signal_done/request_feedback) + per-project server naming (docs/63, 9, 90,
78, 21). *Change:* proxy the Rust core, not REST-over-HTTP.

## 22. Distributed Execution (Claim/Lease) — **keep core, drop pool** (build: HS2-11)
Keep: claim/lease columns + atomic claim-next + blocked-by gate + auto-claim actor +
poison dead-letter (docs/90). **Drop (retired HS-9686):** worker pool, dynamic
scaling, coordinator-dispatch, batching, worker git-state (docs/91, 92, 98–102).
Reconcile with human assignment (docs/10).

## 23. Git Worktrees, Git Status & Code Review — **keep-with-changes**
Keep: git-status chip/popover (docs/48); follower `.hotsheet` redirect (docs/89).
**Drop (retired):** worktree create/list + per-worktree agents, node_modules
provisioning, integration helpers (docs/89, 105, 106). *Decide:* Glassbox review-note
inducement + proof artifacts (docs/110, 111, 122) — port or drop?

## 24. Telemetry / OTLP / Cost — **keep-with-changes** (defer → HS2-17)
OTLP receiver, cost widget, per-ticket attribution, tracing, retention, foreign-OTLP
filter (docs/67, 68, 85, 74, 127). *Decide:* on-by-default? Claude-only today; verify
value vs. complexity. Likely post-v1.

## 25. Analytics & Stats Dashboards — **keep-with-changes** (defer → HS2-17)
Per-project stats (throughput/CFD/cycle-time), Claude-usage analytics, cross-project
stats, inline SVG charts (docs/4 §4.15, 70, 71). Nice; post-floor.

## 26. Announcer (AI Narration + TTS) — **reconsider** (defer → HS2-17)
Digest PIP, live mode, mid-task narration, cross-project reel, multi-provider TTS,
diff visuals (docs/78, 80, 81, 82). Beta, key-consuming — decide if it's part of the
product identity or an optional add-on.

## 27. Backups, Snapshots & Repair — **mostly drop** (git replaces it)
Tiered backups, preview/safety restore, snapshot protection, repair panel,
empty-cluster surfacing, PGLite robustness (docs/7, 41, 42, 44, 45, 72, 73, 135).
*Change:* **git history IS the backup** — most of this subsystem is obsoleted
(docs/02 §2.9). Keep only: instance locking (index writer) + an index rebuild
("repair" = reindex).

## 28. Remote Access & Multi-Client — **keep-with-changes / defer** (design carried)
Service/client decoupling, WS push sync, mTLS + per-client certs, request hardening,
self-hosting, remote-client tab mounting (docs/46, 93, 94, 96, 97, 112). *Change:*
carried over as-is (docs/04 §4.6, docs/08); phase after the local floor.

## 29. Desktop App (Tauri) — **core-keep** (build: HS2-12)
Tauri wrapper, auto-update, install-to-PATH, server supervision, menus (docs/10,
134). *Change:* Tauri **launches + supervises an independent local server** that
**survives app close** (no Node sidecar tied to the app, no embedded core —
docs/09 §9.1e). Dropped: app-icon variants (docs/13, removed).

## 30. Notifications — **keep-with-changes**
In-app toasts, native OS notifications when backgrounded, attention notifications,
bell indicators (docs/27, 30, 4 §4.17, 23, 24). Port; feeds human-assignment
attention (docs/10 §10.3).

## 31. Plugins & External Sync — **reconsider** (assess demand)
ESM plugin format + manifest, bidirectional sync engine, GitHub Issues plugin,
plugin UI hooks, scheduled sync, conflict UI (docs/18, 19, 88). *Decide:* with git
storage + GitHub-backed stores, is a separate GitHub-Issues sync plugin still
wanted, or does git-native sharing cover it?

## 32. Secure Storage, Keychain & API Keys — **core-keep**
OS-keychain secure storage w/ fallback, global API-key registry, transparent setting
backend (docs/20, 79). Port (needed for mTLS certs + provider keys).

## 33. CLI, Server & Isolation — **core-keep** (build: HS2-7/8)
`hotsheet` CLI flags, Hono server hosting, isolated test instance, cluster cache /
memory-pressure budgeting (docs/8, 1, 87, 128, 131). *Change:* Rust server/CLI over
the core; cluster-memory items are PGLite-specific and drop.

## 34. Database & Storage Internals — **replace** (build: HS2-4/5)
PGLite, inline SQL migrations, `.hotsheet/` layout, sibling telemetry clusters, WAL
mitigations (docs/1, 2, 45, 127). **Wholesale replaced** by git stores + SQLite index
(docs/02, 03).

## 35. Client Architecture & Rendering Internals — **replace** (build: HS2-12)
Custom server-JSX + no-React client, kerfjs signals, composable stores, unified
render targets, project-scoped state guard (docs/1, 60, 61, 62, 126). *Change:* HS2
clients are native/SPA over a data API; recommend a small reactive framework, not a
hand-rolled runtime (docs/06 §6.3).

## 36. Print, Export & Sharing — **keep-with-changes**
Print (dashboard/all/selected/individual, multiple formats), ticket cross-reference
modal, Web-Share prompt + footer link (docs/4, 55, 17). *Decide:* print keep; the
share-prompt/virality feature — reconsider.

---

## Cross-cutting notes
- **Retired in HS1 (likely-drop):** the parallel worker pool / worktree
  orchestration (docs/91–106 except claim/lease core + follower pointer), removed
  HS-9686.
- **Removed:** app-icon variants (docs/13).
- **Superseded (read the successor):** docs/69→70, docs/123→129, docs/38→39.
- **Design-only in HS1:** docs/44, 72; docs/46/112 (remote, largely design — but the
  mTLS server half shipped).

## Cross-references
- Build tickets already covering areas: HS2-3…HS2-14, HS2-18 (see [README](README.md)).
- Deferred long-tail umbrella: **HS2-17**.
- This survey's decision tickets: children of **HS2-22**.
