<!-- hotsheet:begin section=claude-adapter v=1 -->
## Shared Project Guidance (CLAUDE.md)

`CLAUDE.md` is the shared source of truth for this repository's engineering rules. Read it completely before making or reviewing changes, and follow it as if its contents appeared here. The filename reflects the project's history; the instructions apply equally to this tool.

- Project workflows are exposed as skills under `.agents/skills/`. Use a skill when the user names it or the request clearly matches its description.
- The skill adapters delegate to `.claude/skills/`, the canonical source for workflows shared across AI tools. When changing a shared workflow, edit the canonical file and keep the adapter metadata in sync.
- Claude tool names in shared documents describe capabilities, not required product-specific tools — use this tool's equivalent file-search, shell, editing, or web capability.
- Keep durable repository guidance in `CLAUDE.md`; provider-specific configuration belongs in its provider's directory.
<!-- hotsheet:end section=claude-adapter -->

<!-- BEGIN hotsheet:codex -->
## Hot Sheet — ticket workflow

This project tracks work as **Hot Sheet** tickets (plain files under the store). Use
them to know what to do next and to record what you did. Everything below works
**headless** — no app, and no server required.

**Find and plan the complete queue:**
- `hotsheet-cli ls --up-next` — the prioritized Up Next queue.
- `hotsheet-cli show <slug>` — read one ticket in full (e.g. `hotsheet-cli show HS-7F3K9Q`).
- Or the MCP tools: `hotsheet_query` (with `up_next: true`) and `hotsheet_get`.

**Do the work, and record progress on the ticket as you go:**
- `hotsheet-cli edit <slug> --status started` when you begin.
- `hotsheet-cli edit <slug> --status completed --note "what you did"` when done.
- Or `hotsheet_update` (it takes a `note`) / `hotsheet_close` through MCP.

**Create tickets for new work you discover** (bugs, follow-ups, gaps) rather than
leaving them in comments:
- `hotsheet-cli new --title "…" --category bug` — or the `hotsheet_create` MCP tool.

Normally continue until every actionable Up Next ticket is complete. Read the whole queue
before choosing an order; consider dependencies, overlap, shared context, risk, and safe
parallelization. Treat priority as important guidance rather than a hard ordering rule.
The CLI and MCP tools use the same engine, so use whichever is handier.
<!-- END hotsheet:codex -->
