---
id: 01M0H6M3SHWGBAKGV5K96GRNX6
slug: HS2-JXZMG1
title: start planning the ideas for Hot Sheet 2
category: task
priority: default
status: completed
created_at: 2026-08-18T23:15:43.821Z
updated_at: 2026-08-19T00:56:05.126Z
completed_at: 2026-08-19T00:56:05.126Z
closed_at: 2026-08-19T00:56:05.126Z
close_reason: completed
legacy_number: HS2-1
schema: 1
---

I want to do a major rewrite of Hot Sheet (see ~/Documents/hotsheet) focusing on simplifying the overall architecture, separating the client and service aspects, and making it more maintainable going forward.

Major changes:
- instead of using pglite to store tickets, I want to use git repos.  Each project should point to one or more git repos used to store hot sheet issues.  
- projects may want multiple ticket repos because:
  - some tickets may need different access permissions, enforced by tools like github
  - some tickets may be single user and/or local-only
- we should make a separate script for converting pglite-based hot sheet tickets into git repo-based tickets.  it should be automatically run (after prompting for confirmation in the UI)
- need to be able index / query / interact with tickets efficiently so we need at least some kind of indexing system, possibly sqlite, but you can help propose other options.  for example, i need to be able to search the text of tickets efficiently and we also need to be able to draw tickets and their information efficiently, so we cant necessarily be loading from disk in real time for every UI operation
- we should probably consider writing the server in go or rust.  the cli tool(s) should also probably be in go or rust
- in addition to a server used for live querying the indexed data, there should be a cli for direct reading / manipulation of tickets on disk.  changes on disk should be automatically monitored for the server to reindex when its running
- AI tools can use the cli and/or the MCP to access tickets
- the server should run completely independently of the client
- ticket numbering should be more uuid / random based instead of linearly increasing
- ai tool integration should be entirely plugin based with no single first class ai tool support -- but we should have a general interface for:
  - initializing ai tools in terminals including setting up MCP and similar connections
  - listing ai tool connections
  - triggering commands to a target connection
  - dealing with permissions checks and other types of user prompts
  - keeping track of ai tool busy-ness
  - evaluate if there are other ai tool interface concepts we should carry over from ~/Documents/hotsheet
- for the client, we will offer a tauri-based client and also a native swift-ui based client for macos and ios
- we may also want a native android client later

Open Questions:
- need to think about how projects can be run on distributed systems, orchestrated through a single ui
- how mobile devices (iOS) can / should be configured and connected to one or more servers

this should be a completely fresh implementation

this first step is just about thinking about the design, writing design docs, and making core technology decisions

## Notes

<!-- note: 01M0H6M3SJ1M137YPW1MBCTRYW -->
2026-08-19T00:25:04.597Z — FEEDBACK NEEDED: Hot Sheet 2 design is drafted. Please review + confirm the core tech decisions before implementation.

**Delivered** — 10 design docs in `docs/` (read `docs/README.md` first):
- `00` vision/principles · `01` architecture · `02` git-repo ticket storage · `03` SQLite+FTS5 index · `04` core/server/CLI · `05` AI-tool plugins · `06` clients · `07` migration · `08` distributed/remote · `09` **technology decisions (ADR log)**.
- Roadmap filed as backlog tickets **HS2-2 … HS2-17**.

**Core decisions made (recommendations):**
1. **Storage:** git repos, one Markdown+YAML file per ticket, multiple stores per project (mixed permissions/locality). Git = source of truth.
2. **IDs:** ULID + short derived slug (`HS-7f3k9q`) — no central sequence, offline-safe.
3. **Index:** SQLite + FTS5, disposable/rebuildable from git.
4. **Language (the pivotal, expensive-to-reverse call — HS2-2):** a shared **Rust** core reused by server, CLI, Tauri, and native SwiftUI/iOS (via uniffi). **Go** is the documented fallback if you'd rather trade native-client core-sharing for faster iteration.
5. **Clients:** Tauri (to feature floor first) + native SwiftUI macOS/iOS; Android later.
6. **AI tools:** plugin-only, no first-class tool (starts from HS1's docs/132 end-state).

**Please confirm or redirect:**
- **(A) Language: Rust (recommended) or Go?** — this gates all implementation.
- **(B) Storage/ID/index direction** as above — any objections?
- **(C) Client sequencing** — Tauri-first, then SwiftUI? macOS+iOS together or iOS after?
- **(D)** Notes inline-in-file vs per-note files (recommend inline first).

I've left this ticket `started` pending your sign-off; once you confirm the language I'll fill in the test/CI setup (HS2-16) and the build can begin.

<!-- note: 01M0H6M3SJ5FRYX1DH248ZND2D -->
2026-08-19T00:51:07.104Z — > Hot Sheet 2 design is drafted. Please review + confirm the core tech decisions before implementation.

> **Delivered** — 10 design docs in `docs/` (read `docs/README.md` first):

> - `00` vision/principles · `01` architecture · `02` git-repo ticket storage · `03` SQLite+FTS5 index · `04` core/server/CLI · `05` AI-tool plugins · `06` clients · `07` migration · `08` distributed/remote · `09` **technology decisions (ADR log)**.

> - Roadmap filed as backlog tickets **HS2-2 … HS2-17**.

> **Core decisions made (recommendations):**

> 1. **Storage:** git repos, one Markdown+YAML file per ticket, multiple stores per project (mixed permissions/locality). Git = source of truth.

great + attachments support

needs to be almost entirely automatic in terms of conflict resolution

> 2. **IDs:** ULID + short derived slug (`HS-7f3k9q`) — no central sequence, offline-safe.

all caps though

> 3. **Index:** SQLite + FTS5, disposable/rebuildable from git.

great

> 4. **Language (the pivotal, expensive-to-reverse call — HS2-2):** a shared **Rust** core reused by server, CLI, Tauri, and native SwiftUI/iOS (via uniffi). **Go** is the documented fallback if you'd rather trade native-client core-sharing for faster iteration.

rust sounds good

> 5. **Clients:** Tauri (to feature floor first) + native SwiftUI macOS/iOS; Android later.

tauri + web first, then native swift ui for macos, then native swiftui for ios, and then kotlin or whatever for android

> 6. **AI tools:** plugin-only, no first-class tool (starts from HS1's docs/132 end-state).

yep

> **Please confirm or redirect:**

> - **(A) Language: Rust (recommended) or Go?** — this gates all implementation.

rust

> - **(B) Storage/ID/index direction** as above — any objections?

sounds good

> - **(C) Client sequencing** — Tauri-first, then SwiftUI? macOS+iOS together or iOS after?

i added notes above

> - **(D)** Notes inline-in-file vs per-note files (recommend inline first).

inline is fine, probably with timestamp oriented uuids for good automatic conflict resolution

> I've left this ticket `started` pending your sign-off; once you confirm the language I'll fill in the test/CI setup (HS2-16) and the build can begin.

<!-- note: 01M0H6M3SJNC8824GKEQWNDKF2 -->
2026-08-19T00:56:05.126Z — **TL;DR:** Hot Sheet 2 design phase complete. 10 design docs in `docs/`, all core technology decisions **made and confirmed by the maintainer**, and a full implementation roadmap filed as tickets HS2-2…HS2-18.

### Deliverables
- **Design docs** (`docs/README.md` is the index): `00` vision/principles · `01` architecture · `02` git-repo storage · `03` SQLite+FTS5 index · `04` core/server/CLI · `05` AI-tool plugins · `06` clients · `07` migration · `08` distributed/remote · `09` technology-decision ADR log.
- **CLAUDE.md** docs-layout marker filled in.

### Confirmed core decisions (maintainer, 2026-08-19)
- **Language:** shared **Rust** core (`hotsheet-core`) reused by server, CLI, Tauri, and native SwiftUI/iOS via `uniffi`. *(HS2-2, resolved.)*
- **Storage:** git repos, one Markdown+YAML file per ticket, multiple stores per project (mixed permissions/locality), **attachments** first-class. Git = source of truth.
- **IDs:** ULID + short **ALL-CAPS** slug (`HS-7F3K9Q`); note ids are ULIDs too.
- **Automatic conflict resolution** (maintainer's key addition): a format-aware **semantic git merge driver** — frontmatter merged field-by-field, notes unioned by timestamp-ordered UUID, only same-paragraph body edits can still conflict. *(HS2-18, docs/02 §2.7.)*
- **Index:** SQLite + FTS5, disposable/rebuildable from git.
- **AI tools:** plugin-only, no first-class tool (starts from HS1 docs/132 end-state).
- **Client order:** Tauri+web → SwiftUI macOS → SwiftUI iOS → Android (Kotlin/Compose).

### Follow-ups filed
- **Implementation:** HS2-3 (core+model), HS2-4 (git store), HS2-5 (index), HS2-6 (watcher), HS2-7 (server), HS2-8 (CLI), HS2-9 (plugin host+Claude), HS2-10 (terminals), HS2-11 (permissions+claims), HS2-12 (Tauri+web), HS2-13 (SwiftUI macOS→iOS), HS2-14 (migration), HS2-18 (merge driver), HS2-16 (test/CI setup).
- **Deferred/open:** HS2-15 (remaining implementation-time decisions — D1/D2/O3/O4/O5/O6), HS2-17 (HS1 feature long-tail).

No blockers remain; implementation can begin with the core scaffold (HS2-3) on Rust.
