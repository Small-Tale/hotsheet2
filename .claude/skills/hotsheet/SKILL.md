---
name: hotsheet
description: Work the Hot Sheet ticket queue for this project — find Up Next tickets, implement them in priority order, and record progress. Works headless (no app), with or without a server.
allowed-tools: Read, Grep, Glob, Edit, Write, Bash
---

Work the project's Hot Sheet ticket queue in priority order.

1. **Read the queue.** `hotsheet-cli ls --up-next` shows the prioritized Up Next
   tickets. (Equivalently, the `hotsheet_query` MCP tool with `up_next: true`.)
2. **Pick the top ticket** and read it in full: `hotsheet-cli show <slug>` (or
   `hotsheet_get`). Understand what "done" means before you start.
3. **Mark it started**, implement it, then **mark it done** with a note:
   - `hotsheet-cli edit <slug> --status started`
   - …do the work…
   - `hotsheet-cli edit <slug> --status completed --note "what you did"`
   - (Or `hotsheet_update` — it takes a `note` — / `hotsheet_close` via MCP.)
4. **File follow-ups** for anything you discover but don't finish (gaps, bugs,
   TODOs): `hotsheet-cli new --title "…" --category <bug|task|feature>` (or
   `hotsheet_create`). Don't leave loose ends only in code comments.
5. Move to the next Up Next ticket and repeat, honoring priority order.

Notes:
- The CLI (`hotsheet-cli …`) and the `hotsheet_*` MCP tools run the same engine — use
  whichever is convenient. Both work with no server running.
- **Confirm the generation before using a connected MCP.** `hotsheet-store.json` (directly
  or through a `.hotsheet/store` link) identifies an HS2 git store; `.hotsheet/db/PG_VERSION`
  identifies an HS1 PGLite project. If the repo has HS1 data but its tickets live in a
  separate HS2 store, an already-connected `hotsheet_*` MCP may still belong to HS1. Use
  `hotsheet-cli -C <HS2-store>` until the MCP endpoint identifies itself as HS2. A 401/403
  alone does not prove the secret is stale; it may be the wrong generation or project.
- If a ticket is unclear or blocked, file a follow-up ticket describing the blocker
  (`hotsheet-cli new` / `hotsheet_create`) and move on rather than guessing.
